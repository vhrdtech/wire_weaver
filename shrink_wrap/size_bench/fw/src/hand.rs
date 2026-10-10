//! The `busgen` case with a hand-written codec in place of shrink_wrap: the reader and writer of busgen_proto
//! (bytes, little endian, a list is a count and its items, reading past the end gives zeros) and the same
//! request and reply enums. What the same job costs when written by hand.

use crate::types::{I2cSlave, I2cTiming, RxFrame, UartFormat, Wave};
use core::hint::black_box;

fn src<T>(v: T) -> T {
    black_box(v)
}

fn sink<T>(v: T) {
    black_box(&v);
}

/// Reader over a message; past the end it reads zeros.
#[derive(Clone, Copy)]
pub struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn u8(&mut self) -> u8 {
        let v = self.b.get(self.p).copied().unwrap_or(0);
        self.p += 1;
        v
    }
    fn u16(&mut self) -> u16 {
        self.u8() as u16 | (self.u8() as u16) << 8
    }
    fn u32(&mut self) -> u32 {
        self.u16() as u32 | (self.u16() as u32) << 16
    }
    fn bool(&mut self) -> bool {
        self.u8() != 0
    }
    /// A list of `n` items of `size` bytes each: a reader at its first item
    fn list(&mut self, size: usize) -> List<'a> {
        let n = self.u16();
        let at = *self;
        self.p += n as usize * size;
        List { rd: at, n }
    }
}

/// A list in the message, read item by item
#[derive(Clone, Copy)]
pub struct List<'a> {
    rd: Rd<'a>,
    n: u16,
}

/// Writer into a buffer; `full` once something didn't fit.
pub struct Wr<'a> {
    b: &'a mut [u8],
    p: usize,
    full: bool,
}

impl Wr<'_> {
    fn u8(&mut self, v: u8) {
        match self.b.get_mut(self.p) {
            Some(b) => {
                *b = v;
                self.p += 1;
            }
            None => self.full = true,
        }
    }
    fn u16(&mut self, v: u16) {
        self.u8(v as u8);
        self.u8((v >> 8) as u8);
    }
    fn u32(&mut self, v: u32) {
        self.u16(v as u16);
        self.u16((v >> 16) as u16);
    }
    fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
}

fn wave(r: &mut Rd) -> Wave {
    Wave {
        line: r.u8(),
        idle: r.bool(),
        hold: r.bool(),
        xor: r.bool(),
        delay: r.u32(),
        high: r.u32(),
        low: r.u32(),
        n: r.u32(),
    }
}

fn uart_format(r: &mut Rd) -> UartFormat {
    UartFormat {
        bit: r.u32(),
        bits: r.u8(),
        parity: r.u8(),
        stop: r.u32(),
        msb_first: r.bool(),
        inv: r.bool(),
    }
}

pub struct Utx<'i> {
    ch: u8,
    line: u8,
    fmt: UartFormat,
    gap: u32,
    delay: u32,
    off: u16,
    n: u16,
    brklen: u32,
    trunc: u8,
    glitch: Option<Wave>,
    perr: List<'i>,
    ferr: List<'i>,
    brk: List<'i>,
    defer: bool,
}

pub struct I2c<'i> {
    scl: u8,
    sda: u8,
    log: bool,
    t: I2cTiming,
    delay: u32,
    off: u16,
    n: u16,
    glitches: List<'i>,
    pp: u8,
    keep_log: bool,
    defer: bool,
}

pub enum Request<'i> {
    Hello,
    Status,
    Pins,
    Reset,
    Stop {
        mask: u16,
    },
    Release {
        lines: u8,
    },
    Mark {
        on: bool,
        width: u32,
        lead: u32,
        line: u8,
    },
    Pull {
        line: u8,
        up: u8,
        down: u8,
    },
    Waves {
        waves: List<'i>,
        defer: bool,
    },
    Go,
    Wait {
        timeout_ms: u32,
    },
    Load {
        off: u16,
        frames: List<'i>,
    },
    Dump {
        off: u16,
        n: u16,
    },
    Utx(Utx<'i>),
    Urx {
        line: u8,
        fmt: UartFormat,
        defer: bool,
    },
    UrxGet {
        max: u16,
        flush: bool,
    },
    I2c(I2c<'i>),
    I2cSlave {
        s: I2cSlave,
        keep_log: bool,
        defer: bool,
    },
}

const WAVE_SIZE: usize = 20;

impl<'i> Request<'i> {
    fn read(b: &'i [u8]) -> Option<Request<'i>> {
        let mut r = Rd { b, p: 0 };
        let r = &mut r;
        Some(match r.u8() {
            0 => Request::Hello,
            1 => Request::Status,
            2 => Request::Pins,
            3 => Request::Reset,
            4 => Request::Stop { mask: r.u16() },
            5 => Request::Release { lines: r.u8() },
            6 => Request::Mark {
                on: r.bool(),
                width: r.u32(),
                lead: r.u32(),
                line: r.u8(),
            },
            7 => Request::Pull {
                line: r.u8(),
                up: r.u8(),
                down: r.u8(),
            },
            8 => Request::Waves {
                waves: r.list(WAVE_SIZE),
                defer: r.bool(),
            },
            9 => Request::Go,
            10 => Request::Wait {
                timeout_ms: r.u32(),
            },
            11 => Request::Load {
                off: r.u16(),
                frames: r.list(2),
            },
            12 => Request::Dump {
                off: r.u16(),
                n: r.u16(),
            },
            13 => Request::Utx(Utx {
                ch: r.u8(),
                line: r.u8(),
                fmt: uart_format(r),
                gap: r.u32(),
                delay: r.u32(),
                off: r.u16(),
                n: r.u16(),
                brklen: r.u32(),
                trunc: r.u8(),
                glitch: {
                    let (has, w) = (r.bool(), wave(r));
                    has.then_some(w)
                },
                perr: r.list(2),
                ferr: r.list(2),
                brk: r.list(2),
                defer: r.bool(),
            }),
            14 => Request::Urx {
                line: r.u8(),
                fmt: uart_format(r),
                defer: r.bool(),
            },
            15 => Request::UrxGet {
                max: r.u16(),
                flush: r.bool(),
            },
            16 => Request::I2c(I2c {
                scl: r.u8(),
                sda: r.u8(),
                log: r.bool(),
                t: I2cTiming {
                    hd: r.u32(),
                    lo: r.u32(),
                    hi: r.u32(),
                    su: r.u32(),
                    buf: r.u32(),
                    unit: r.u32(),
                },
                delay: r.u32(),
                off: r.u16(),
                n: r.u16(),
                glitches: r.list(WAVE_SIZE),
                pp: r.u8(),
                keep_log: r.bool(),
                defer: r.bool(),
            }),
            17 => Request::I2cSlave {
                s: {
                    let (on, scl, sda, log, stretch, stretch_c, filt, nack_at) = (
                        r.bool(),
                        r.u8(),
                        r.u8(),
                        r.u8(),
                        r.u8(),
                        r.u32(),
                        r.u8(),
                        r.u16(),
                    );
                    let (has10, a10, hasp, ptr) = (r.bool(), r.u16(), r.bool(), r.u8());
                    I2cSlave {
                        on,
                        scl,
                        sda,
                        log,
                        stretch,
                        stretch_c,
                        filt,
                        nack_at,
                        a10: has10.then_some(a10),
                        ptr: hasp.then_some(ptr),
                    }
                },
                keep_log: r.bool(),
                defer: r.bool(),
            },
            _ => return None,
        })
    }
}

pub enum Reply<'i> {
    Hello {
        proto: u16,
        fw: &'i str,
        clk_hz: u32,
        lines: u8,
        mark_line: u8,
        txbuf: u16,
        rxbuf: u16,
        rx_sync: u8,
        family: u8,
    },
    Ok,
    Run {
        run: u32,
        started: bool,
        mark: u32,
        lead: u32,
        mark_line: u8,
    },
    Status {
        run: u32,
        busy: u16,
        t: u32,
        oe: u8,
        out: u8,
        mark_on: bool,
        mark: u32,
        lead: u32,
        pull_oe: u8,
        pull_out: u8,
        deferred: u16,
        mark_line: u8,
    },
    Pins {
        input: u8,
        oe: u8,
        out: u8,
    },
    Wait {
        run: u32,
        end: u32,
    },
    Dump {
        off: u16,
        frames: &'i [u16],
    },
    UrxGet {
        frames: &'i [RxFrame],
        more: u16,
        overflow: u16,
    },
    Err {
        code: u8,
        arg: u16,
    },
}

impl Reply<'_> {
    fn write(&self, w: &mut Wr) {
        match self {
            Reply::Hello {
                proto,
                fw,
                clk_hz,
                lines,
                mark_line,
                txbuf,
                rxbuf,
                rx_sync,
                family,
            } => {
                w.u8(0);
                w.u16(*proto);
                w.u8(fw.len() as u8);
                for &c in fw.as_bytes() {
                    w.u8(c);
                }
                w.u32(*clk_hz);
                w.u8(*lines);
                w.u8(*mark_line);
                w.u16(*txbuf);
                w.u16(*rxbuf);
                w.u8(*rx_sync);
                w.u8(*family);
            }
            Reply::Ok => w.u8(1),
            Reply::Run {
                run,
                started,
                mark,
                lead,
                mark_line,
            } => {
                w.u8(2);
                w.u32(*run);
                w.bool(*started);
                w.u32(*mark);
                w.u32(*lead);
                w.u8(*mark_line);
            }
            Reply::Status {
                run,
                busy,
                t,
                oe,
                out,
                mark_on,
                mark,
                lead,
                pull_oe,
                pull_out,
                deferred,
                mark_line,
            } => {
                w.u8(3);
                w.u32(*run);
                w.u16(*busy);
                w.u32(*t);
                w.u8(*oe);
                w.u8(*out);
                w.bool(*mark_on);
                w.u32(*mark);
                w.u32(*lead);
                w.u8(*pull_oe);
                w.u8(*pull_out);
                w.u16(*deferred);
                w.u8(*mark_line);
            }
            Reply::Pins { input, oe, out } => {
                w.u8(4);
                w.u8(*input);
                w.u8(*oe);
                w.u8(*out);
            }
            Reply::Wait { run, end } => {
                w.u8(5);
                w.u32(*run);
                w.u32(*end);
            }
            Reply::Dump { off, frames } => {
                w.u8(6);
                w.u16(*off);
                w.u16(frames.len() as u16);
                for &f in *frames {
                    w.u16(f);
                }
            }
            Reply::UrxGet {
                frames,
                more,
                overflow,
            } => {
                w.u8(7);
                w.u16(frames.len() as u16);
                for f in *frames {
                    w.u16(f.d);
                    w.u32(f.t);
                }
                w.u16(*more);
                w.u16(*overflow);
            }
            Reply::Err { code, arg } => {
                w.u8(8);
                w.u8(*code);
                w.u16(*arg);
            }
        }
    }
}

const FRAMES: [u16; 3] = [1, 2, 3];
const RX_FRAMES: [RxFrame; 2] = [RxFrame { d: 1, t: 2 }, RxFrame { d: 3, t: 4 }];

fn reply_run<'i>() -> Reply<'i> {
    Reply::Run {
        run: src(1),
        started: src(true),
        mark: src(2),
        lead: src(3),
        mark_line: src(4),
    }
}

/// The same handler as `cases::handle`
fn handle<'i>(req: &Request<'i>) -> Reply<'i> {
    match req {
        Request::Hello => Reply::Hello {
            proto: src(1),
            fw: src("0.1.0"),
            clk_hz: src(2),
            lines: src(3),
            mark_line: src(4),
            txbuf: src(5),
            rxbuf: src(6),
            rx_sync: src(7),
            family: src(8),
        },
        Request::Status => Reply::Status {
            run: src(1),
            busy: src(2),
            t: src(3),
            oe: src(4),
            out: src(5),
            mark_on: src(true),
            mark: src(6),
            lead: src(7),
            pull_oe: src(8),
            pull_out: src(9),
            deferred: src(10),
            mark_line: src(11),
        },
        Request::Pins => Reply::Pins {
            input: src(1),
            oe: src(2),
            out: src(3),
        },
        Request::Wait { timeout_ms } => {
            sink(timeout_ms);
            Reply::Wait {
                run: src(1),
                end: src(2),
            }
        }
        Request::Dump { off, n } => {
            sink(n);
            Reply::Dump {
                off: *off,
                frames: src(&FRAMES[..]),
            }
        }
        Request::UrxGet { max, flush } => {
            sink((max, flush));
            Reply::UrxGet {
                frames: src(&RX_FRAMES[..]),
                more: src(1),
                overflow: src(2),
            }
        }
        Request::Waves { waves, defer } => {
            let mut rd = waves.rd;
            for _ in 0..waves.n {
                sink(wave(&mut rd));
            }
            sink(defer);
            reply_run()
        }
        Request::Load { off, frames } => {
            sink(off);
            let mut rd = frames.rd;
            for _ in 0..frames.n {
                sink(rd.u16());
            }
            Reply::Ok
        }
        Request::Utx(utx) => {
            for list in [&utx.perr, &utx.ferr, &utx.brk] {
                let mut rd = list.rd;
                for _ in 0..list.n {
                    sink(rd.u16());
                }
            }
            sink((
                utx.ch, utx.line, utx.fmt, utx.gap, utx.delay, utx.off, utx.n, utx.brklen,
                utx.trunc, utx.glitch,
            ));
            sink(utx.defer);
            reply_run()
        }
        Request::I2c(i2c) => {
            let mut rd = i2c.glitches.rd;
            for _ in 0..i2c.glitches.n {
                sink(wave(&mut rd));
            }
            sink((
                i2c.scl,
                i2c.sda,
                i2c.log,
                i2c.t,
                i2c.delay,
                i2c.off,
                i2c.n,
                i2c.pp,
                i2c.keep_log,
                i2c.defer,
            ));
            reply_run()
        }
        Request::Urx { line, fmt, defer } => {
            sink((line, fmt, defer));
            reply_run()
        }
        Request::I2cSlave { s, keep_log, defer } => {
            sink((s, keep_log, defer));
            reply_run()
        }
        Request::Stop { mask } => {
            sink(mask);
            Reply::Ok
        }
        Request::Release { lines } => {
            sink(lines);
            Reply::Ok
        }
        Request::Mark {
            on,
            width,
            lead,
            line,
        } => {
            sink((on, width, lead, line));
            Reply::Ok
        }
        Request::Pull { line, up, down } => {
            sink((line, up, down));
            Reply::Ok
        }
        Request::Reset | Request::Go => Reply::Ok,
    }
}

pub fn run(rx: &[u8], tx: &mut [u8]) -> usize {
    let reply = match Request::read(rx) {
        Some(req) => handle(&req),
        None => Reply::Err {
            code: src(1),
            arg: src(2),
        },
    };
    let mut w = Wr {
        b: tx,
        p: 0,
        full: false,
    };
    reply.write(&mut w);
    if w.full { 0 } else { w.p }
}
