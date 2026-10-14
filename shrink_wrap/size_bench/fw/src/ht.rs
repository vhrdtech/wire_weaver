//! The `busgen` case in the head+tail encoding (SW-46): the same request and reply enums and the same handler,
//! the plain structs fixed-layout, the rest `head_tail`, lists as `head_tail::List`.

use core::hint::black_box;
use shrink_wrap::head_tail::List;
use shrink_wrap::prelude::*;

fn src<T>(v: T) -> T {
    black_box(v)
}

fn sink<T>(v: T) {
    black_box(&v);
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy))]
pub struct Wave {
    pub line: u8,
    pub idle: bool,
    pub hold: bool,
    pub xor: bool,
    pub delay: u32,
    pub high: u32,
    pub low: u32,
    pub n: u32,
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy))]
pub struct UartFormat {
    pub bit: u32,
    pub bits: u8,
    pub parity: u8,
    pub stop: u32,
    pub msb_first: bool,
    pub inv: bool,
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy))]
pub struct I2cTiming {
    pub hd: u32,
    pub lo: u32,
    pub hi: u32,
    pub su: u32,
    pub buf: u32,
    pub unit: u32,
}

#[derive_shrink_wrap(head_tail, derive(Clone, Copy))]
pub struct I2cSlave {
    pub on: bool,
    pub scl: u8,
    pub sda: u8,
    pub log: u8,
    pub stretch: u8,
    pub stretch_c: u32,
    pub filt: u8,
    pub nack_at: u16,
    pub a10: Option<u16>,
    pub ptr: Option<u8>,
}

#[derive_shrink_wrap(head_tail, derive(Clone, Copy))]
pub struct Utx<'i> {
    pub ch: u8,
    pub line: u8,
    pub fmt: UartFormat,
    pub gap: u32,
    pub delay: u32,
    pub off: u16,
    pub n: u16,
    pub brklen: u32,
    pub trunc: u8,
    pub glitch: Option<Wave>,
    pub perr: List<'i, u16>,
    pub ferr: List<'i, u16>,
    pub brk: List<'i, u16>,
    pub defer: bool,
}

#[derive_shrink_wrap(head_tail, derive(Clone, Copy))]
pub struct I2c<'i> {
    pub scl: u8,
    pub sda: u8,
    pub log: bool,
    pub t: I2cTiming,
    pub delay: u32,
    pub off: u16,
    pub n: u16,
    pub glitches: List<'i, Wave>,
    pub pp: u8,
    pub keep_log: bool,
    pub defer: bool,
}

#[derive_shrink_wrap(head_tail, derive(Clone, Copy), ww_repr = u8)]
pub enum Request<'i> {
    Hello,
    Status,
    Pins,
    Reset,
    Stop { mask: u16 },
    Release { lines: u8 },
    Mark { on: bool, width: u32, lead: u32, line: u8 },
    Pull { line: u8, up: u8, down: u8 },
    Waves { waves: List<'i, Wave>, defer: bool },
    Go,
    Wait { timeout_ms: u32 },
    Load { off: u16, frames: List<'i, u16> },
    Dump { off: u16, n: u16 },
    Utx(Utx<'i>),
    Urx { line: u8, fmt: UartFormat, defer: bool },
    UrxGet { max: u16, flush: bool },
    I2c(I2c<'i>),
    I2cSlave { s: I2cSlave, keep_log: bool, defer: bool },
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Clone, Copy))]
pub struct RxFrame {
    pub d: u16,
    pub t: u32,
}

#[derive_shrink_wrap(head_tail, derive(Clone, Copy), ww_repr = u8)]
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
    Run { run: u32, started: bool, mark: u32, lead: u32, mark_line: u8 },
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
    Pins { input: u8, oe: u8, out: u8 },
    Wait { run: u32, end: u32 },
    Dump { off: u16, frames: List<'i, u16> },
    UrxGet { frames: List<'i, RxFrame>, more: u16, overflow: u16 },
    Err { code: u8, arg: u16 },
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
fn handle<'i>(req: &Request<'i>) -> Result<Reply<'i>, ShrinkWrapError> {
    Ok(match req {
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
                frames: List::new(src(&FRAMES[..])),
            }
        }
        Request::UrxGet { max, flush } => {
            sink((max, flush));
            Reply::UrxGet {
                frames: List::new(src(&RX_FRAMES[..])),
                more: src(1),
                overflow: src(2),
            }
        }
        Request::Waves { waves, defer } => {
            for wave in waves.iter() {
                sink(wave?);
            }
            sink(defer);
            reply_run()
        }
        Request::Load { off, frames } => {
            sink(off);
            for frame in frames.iter() {
                sink(frame?);
            }
            Reply::Ok
        }
        Request::Utx(utx) => {
            for list in [&utx.perr, &utx.ferr, &utx.brk] {
                for idx in list.iter() {
                    sink(idx?);
                }
            }
            sink(utx);
            reply_run()
        }
        Request::I2c(i2c) => {
            for wave in i2c.glitches.iter() {
                sink(wave?);
            }
            sink(i2c);
            reply_run()
        }
        Request::Urx { .. } | Request::I2cSlave { .. } => {
            sink(req);
            reply_run()
        }
        other => {
            sink(other);
            Reply::Ok
        }
    })
}

/// The whole firmware codec: a request decoded, acted on, and its reply encoded
pub fn run(rx: &[u8], tx: &mut [u8]) -> usize {
    let reply = match Request::from_head_tail(rx).and_then(|req| handle(&req)) {
        Ok(reply) => reply,
        Err(e) => {
            sink(e);
            Reply::Err {
                code: src(1),
                arg: src(2),
            }
        }
    };
    reply.to_head_tail(tx).unwrap_or(0)
}
