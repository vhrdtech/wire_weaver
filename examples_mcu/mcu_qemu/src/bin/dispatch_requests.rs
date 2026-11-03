//! Instructions per request of the dispatcher (TEST-30): a server of `flash_budget_api::BaseOnly` (the same server
//! as `qemu-flash-base-only`, see `src/flash_budget.rs`) answers one request of each kind, each bracketed by
//! `mark_start`/`mark_end` for `qemu_icount`, Cortex-M3 (`lm3s6965evb`). The first pair is calibration, then each
//! request twice (`run=0` cold, `run=1` warm), in the order of the `bracket=..` lines; then the reply of each, its
//! length and FNV hash, so a change to the dispatcher that changes a reply shows here too (TEST-31 is the proof).
//! `examples_mcu/fw_budget --requests` (`just dispatch-budget`) builds and runs it at `fw-z` and `fw-3`, sync and
//! `use_async` (feature `flash-async`).
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;

use core::fmt::Write;
use core::hint::black_box;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::UVlq32Backfill;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use ww_client_server::{PathKind, Priority, Request, RequestKind, StreamSideband};

flash_budget_server_impl!(BaseOnly);

impl ServerState {
    base_handlers!();
}

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// Indices in `BaseOnly`: `base` 0, and in `Base`: c0 0, c1 1, a0 2, a1 3, p0 4, p1 5, s0 6, s1 7.
const CASES: [(&str, u32); 8] = [
    ("call, no arguments or output", 0),
    ("call, an argument and an output", 2),
    ("property read", 4),
    ("property write", 4),
    ("stream open", 6),
    ("stream close", 6),
    ("error: bad path", 9),
    ("error: kind not taken", 0),
];

/// The request bytes of case `i`, into `buf`.
fn request(i: usize, buf: &mut [u8]) -> &[u8] {
    let mut code = [0u8; 8];
    let code = flash_budget_api::Code(0x1234)
        .to_ww_bytes(&mut code)
        .unwrap();
    let kind = match i {
        0 | 6 => RequestKind::Call {
            args: TailBytes(&[]),
        },
        1 => RequestKind::Call {
            args: TailBytes(code),
        },
        2 | 7 => RequestKind::Read,
        3 => RequestKind::Write {
            data: TailBytes(code),
        },
        4 => RequestKind::StreamOpen {
            resume: None,
            max_age_us: None,
            codecs: None,
            args: TailBytes(&[]),
        },
        _ => RequestKind::StreamSideband {
            sideband: StreamSideband::Close,
        },
    };
    let path = [UNib32(0), UNib32(CASES[i].1)];
    let req = Request {
        seq: UVlq32Backfill(7),
        priority: Priority::NORMAL.nibble(),
        path_kind: PathKind::absolute(&path),
        kind,
    };
    req.to_ww_bytes(buf).unwrap()
}

/// One request through the server, its reply into `scratch`.
#[inline(never)]
fn serve_reply<'a>(request: &[u8], scratch: &'a mut [u8]) -> Option<&'a [u8]> {
    let mut event_scratch = [0u8; 256];
    let mut server = ServerState {};
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    #[cfg(not(feature = "flash-async"))]
    let r = server.process_request_bytes(request, scratch, &mut out, ());
    #[cfg(feature = "flash-async")]
    let r = {
        let mut fut = core::pin::pin!(server.process_request_bytes(request, scratch, &mut out, ()));
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(r) => r,
            core::task::Poll::Pending => Ok(&[][..]),
        }
    };
    r.ok()
}

fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    // serve() is linked too, as in the flash bins, so this ELF's dispatcher is theirs
    black_box(serve(black_box(&[])));
    mark_start();
    mark_end();
    let mut n = 0;
    let mut replies = [([0u8; 64], 0usize); CASES.len()];
    for (i, (label, _)) in CASES.iter().enumerate() {
        let mut buf = [0u8; 64];
        let req = request(i, &mut buf);
        for run in 0..2 {
            writeln!(stdout, "bracket={n} {label} run={run} ops=1").unwrap();
            n += 1;
            let mut scratch = [0u8; 256];
            mark_start();
            let reply = serve_reply(black_box(req), &mut scratch);
            mark_end();
            let reply = reply.unwrap_or(&[]);
            replies[i].0[..reply.len()].copy_from_slice(reply);
            replies[i].1 = reply.len();
        }
    }
    for (i, (label, _)) in CASES.iter().enumerate() {
        let reply = &replies[i].0[..replies[i].1];
        writeln!(
            stdout,
            "{label} | bytes={} hash={:08x} reply={reply:02x?}",
            reply.len(),
            fnv(reply)
        )
        .unwrap();
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
