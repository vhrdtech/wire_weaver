//! Instructions per request of the dispatcher (TEST-30): a server of `flash_budget_api::BaseOnly` (the same server
//! as `qemu-flash-base-only`, see `src/flash_budget.rs`) answers one request of each kind, each bracketed by
//! `mark_start`/`mark_end` for `qemu_icount`, Cortex-M3 (`lm3s6965evb`). The first pair is calibration, then each
//! request twice (`run=0` cold, `run=1` warm), in the order of the `bracket=..` lines; then the reply of each, its
//! length and FNV hash, so a change to the dispatcher that changes a reply shows here too (TEST-31 is the proof).
//! `examples_mcu/fw_budget --requests` (`just dispatch-budget`) builds and runs it at `fw-z` and `fw-3`, sync and
//! `use_async` (feature `flash-async`, which also prints the size of the request's future).
#![no_main]
#![no_std]

extern crate panic_semihosting;

#[macro_use]
#[path = "../flash_budget.rs"]
mod flash_budget;
#[path = "../dispatch_cases.rs"]
mod dispatch_cases;

use core::fmt::Write;
use core::hint::black_box;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use dispatch_cases::{CASES, fnv, request};
use wire_weaver::prelude::*;

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
        let fut = server.process_request_bytes(request, scratch, &mut out, ());
        FUTURE_BYTES.store(
            core::mem::size_of_val(&fut),
            core::sync::atomic::Ordering::Relaxed,
        );
        let mut fut = core::pin::pin!(fut);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            core::task::Poll::Ready(r) => r,
            core::task::Poll::Pending => Ok(&[][..]),
        }
    };
    r.ok()
}

/// The size of `process_request_bytes`' future (`use_async`): the state machine of every level it may await in.
#[cfg_attr(not(feature = "flash-async"), allow(dead_code))]
static FUTURE_BYTES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

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
    #[cfg(feature = "flash-async")]
    writeln!(
        stdout,
        "future bytes={}",
        FUTURE_BYTES.load(core::sync::atomic::Ordering::Relaxed)
    )
    .unwrap();
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
