//! The test cases: one `run` per feature.

#![allow(unused_imports, dead_code)]

use crate::types::*;
use core::hint::black_box;
use shrink_wrap::prelude::*;

/// A value the compiler knows nothing about
fn src<T>(v: T) -> T {
    black_box(v)
}

/// Counts as a use of every part of `v`
fn sink<T>(v: T) {
    black_box(&v);
}

fn enc<T: SerializeShrinkWrap>(v: &T, tx: &mut [u8]) -> usize {
    match v.to_ww_bytes(tx) {
        Ok(bytes) => bytes.len(),
        Err(e) => {
            sink(e);
            0
        }
    }
}

const WAVE: Wave = Wave {
    line: 1,
    idle: true,
    hold: false,
    xor: true,
    delay: 2,
    high: 3,
    low: 4,
    n: 5,
};
const FRAMES: [u16; 3] = [1, 2, 3];
const RX_FRAMES: [RxFrame; 2] = [RxFrame { d: 1, t: 2 }, RxFrame { d: 3, t: 4 }];

/// No shrink_wrap code at all: the image the other cases are compared with
#[cfg(feature = "empty")]
pub fn run(rx: &[u8], tx: &mut [u8]) -> usize {
    tx[0] = rx.len() as u8;
    1
}

/// `BufReader` alone: bytes, a flag and little endian integers
#[cfg(feature = "rw_de")]
pub fn run(rx: &[u8], _tx: &mut [u8]) -> usize {
    fn read(rd: &mut BufReader) -> Result<(u8, bool, u16, u32), ShrinkWrapError> {
        Ok((
            rd.read_u8()?,
            rd.read_bool()?,
            rd.read_u16()?,
            rd.read_u32()?,
        ))
    }
    sink(read(&mut BufReader::new(rx)));
    0
}

/// `BufWriter` alone
#[cfg(feature = "rw_ser")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    fn write<'i>(wr: &'i mut BufWriter) -> Result<&'i [u8], ShrinkWrapError> {
        wr.write_u8(src(1))?;
        wr.write_bool(src(true))?;
        wr.write_u16(src(2))?;
        wr.write_u32(src(3))?;
        wr.finish()
    }
    match write(&mut BufWriter::new(tx)) {
        Ok(bytes) => bytes.len(),
        Err(e) => {
            sink(e);
            0
        }
    }
}

#[cfg(feature = "struct_de")]
pub fn run(rx: &[u8], _tx: &mut [u8]) -> usize {
    sink(Wave::from_ww_bytes(rx));
    0
}

#[cfg(feature = "struct_ser")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    enc(&src(WAVE), tx)
}

#[cfg(feature = "enum_de")]
pub fn run(rx: &[u8], _tx: &mut [u8]) -> usize {
    sink(Small::from_ww_bytes(rx));
    0
}

#[cfg(feature = "enum_ser")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    enc(
        &src(Small::Mark {
            on: true,
            width: 1,
            lead: 2,
            line: 3,
        }),
        tx,
    )
}

#[cfg(feature = "vec_de")]
pub fn run(rx: &[u8], _tx: &mut [u8]) -> usize {
    match Load::from_ww_bytes(rx) {
        Ok(load) => {
            sink(load.off);
            for frame in load.frames.iter() {
                sink(frame);
            }
        }
        Err(e) => sink(e),
    }
    0
}

#[cfg(feature = "vec_ser")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    enc(
        &Load {
            off: src(1),
            frames: RefVec::Slice {
                slice: src(&FRAMES[..]),
            },
        },
        tx,
    )
}

#[cfg(feature = "option_de")]
pub fn run(rx: &[u8], _tx: &mut [u8]) -> usize {
    sink(I2cSlave::from_ww_bytes(rx));
    0
}

#[cfg(feature = "option_ser")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    let slave = I2cSlave {
        on: true,
        scl: 1,
        sda: 2,
        log: 3,
        stretch: 4,
        stretch_c: 5,
        filt: 6,
        nack_at: 7,
        a10: Some(8),
        ptr: None,
    };
    enc(&src(slave), tx)
}

fn reply_hello<'i>() -> Reply<'i> {
    Reply::Hello {
        proto: src(1),
        fw: src("0.1.0"),
        clk_hz: src(2),
        lines: src(3),
        mark_line: src(4),
        txbuf: src(5),
        rxbuf: src(6),
        rx_sync: src(7),
        family: src(8),
    }
}

fn reply_status<'i>() -> Reply<'i> {
    Reply::Status {
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
    }
}

fn reply_run<'i>() -> Reply<'i> {
    Reply::Run {
        run: src(1),
        started: src(true),
        mark: src(2),
        lead: src(3),
        mark_line: src(4),
    }
}

fn reply_dump<'i>(off: u16) -> Reply<'i> {
    Reply::Dump {
        off,
        frames: RefVec::Slice {
            slice: src(&FRAMES[..]),
        },
    }
}

fn reply_urx_get<'i>() -> Reply<'i> {
    Reply::UrxGet {
        frames: RefVec::Slice {
            slice: src(&RX_FRAMES[..]),
        },
        more: src(1),
        overflow: src(2),
    }
}

/// Acts on a request as a firmware does: every field used, every list walked, one reply
fn handle<'i>(req: &Request<'i>) -> Result<Reply<'i>, ShrinkWrapError> {
    Ok(match req {
        Request::Hello => reply_hello(),
        Request::Status => reply_status(),
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
            reply_dump(*off)
        }
        Request::UrxGet { max, flush } => {
            sink((max, flush));
            reply_urx_get()
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

/// The firmware's receive side only: a request decoded and acted on
#[cfg(feature = "busgen_de")]
pub fn run(rx: &[u8], _tx: &mut [u8]) -> usize {
    match Request::from_ww_bytes(rx) {
        Ok(req) => sink(handle(&req).is_ok()),
        Err(e) => sink(e),
    }
    0
}

/// The firmware's transmit side only: one of the replies encoded
#[cfg(feature = "busgen_ser")]
pub fn run(_rx: &[u8], tx: &mut [u8]) -> usize {
    let reply = match src(0u8) {
        0 => reply_hello(),
        1 => Reply::Ok,
        2 => reply_run(),
        3 => reply_status(),
        4 => Reply::Pins {
            input: src(1),
            oe: src(2),
            out: src(3),
        },
        5 => Reply::Wait {
            run: src(1),
            end: src(2),
        },
        6 => reply_dump(src(1)),
        7 => reply_urx_get(),
        _ => Reply::Err {
            code: src(1),
            arg: src(2),
        },
    };
    enc(&reply, tx)
}

/// The whole firmware codec: a request decoded, acted on, and its reply encoded
#[cfg(feature = "busgen")]
pub fn run(rx: &[u8], tx: &mut [u8]) -> usize {
    let reply = match Request::from_ww_bytes(rx).and_then(|req| handle(&req)) {
        Ok(reply) => reply,
        Err(e) => {
            sink(e);
            Reply::Err {
                code: src(1),
                arg: src(2),
            }
        }
    };
    enc(&reply, tx)
}

#[cfg(feature = "busgen_hand")]
pub use crate::hand::run;
