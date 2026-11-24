//! A firmware's mount prefix rewrite (SRV-77, SRV-80): `MountPrefix<PathArray<8>>` puts a slot's path in front of a
//! backend's event, without a heap. `rewrite` and `decode` (the `Event` decoding alone) are kept out of line so
//! their flash and frames read off the ELF (`-Z emit-stack-sizes`, `llvm-readobj --stack-sizes`).
//!
//! Two events go through both: an observe update without `sample_times`, and one with three sample times. Each call
//! is bracketed by `mark_start` / `mark_end` for `qemu_icount`: the first bracket is the empty calibration pair,
//! then `decode` and `rewrite` of the first event, then of the second. Each call also prints its stack high-water
//! mark below `main`'s frame, measured by painting the stack below it, and the size of a decoded `Event` comes first.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::shrink_wrap::series::Series;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, RefVec, SerializeShrinkWrap, UVlq32};
use ww_client_server::mount::{MountPrefix, PathArray, RewriteError};
use ww_client_server::{Event, EventKind, ResourceIndex, resource_path};
use ww_si::Second;

/// Size of the event buffer.
const MESSAGE_LEN_MAX: usize = 64;
/// The longest joined path the firmware forwards.
const PATH_LEN_MAX: usize = 8;
/// Bytes of stack painted below `main`'s frame before each measured call: more than any call here takes.
const PAINT_BYTES: usize = 16 * 1024;
/// The paint pattern.
const PAINT: u32 = 0xA5A5_A5A5;

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().expect("semihosting stdout");
    let slot: [ResourceIndex; 2] = resource_path([3, 1]);
    let mut prefix =
        MountPrefix::new(PathArray::<PATH_LEN_MAX>::new(), &slot).expect("the slot path fits");
    let below: [ResourceIndex; 2] = resource_path([0, 4]);
    let times = [
        Second::micro(1_000u32),
        Second::micro(1_250u32),
        Second::micro(1_500u32),
    ];
    let mut plain = [0u8; MESSAGE_LEN_MAX];
    let plain = update(&below, None)
        .to_ww_bytes(&mut plain)
        .expect("the event fits");
    let mut timed = [0u8; MESSAGE_LEN_MAX];
    let timed = update(&below, Some(Series::new(&times)))
        .to_ww_bytes(&mut timed)
        .expect("the event fits");

    writeln!(stdout, "Event: {} B", core::mem::size_of::<Event>()).expect("semihosting write");
    mark_start();
    mark_end();
    for (name, event) in [("plain", plain), ("sample_times", timed)] {
        let top = paint();
        mark_start();
        let decoded = decode(core::hint::black_box(event));
        mark_end();
        let decode_stack = high_water(top);
        let mut buf = [0u8; MESSAGE_LEN_MAX];
        let top = paint();
        mark_start();
        let r = rewrite(&mut prefix, core::hint::black_box(event), &mut buf).map(<[u8]>::len);
        mark_end();
        let rewrite_stack = high_water(top);
        writeln!(
            stdout,
            "{name}: {} B, decode {decoded} stack {decode_stack} B, rewrite {r:?} stack {rewrite_stack} B",
            event.len()
        )
        .expect("semihosting write");
    }

    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}

/// An observe update at `path` with `sample_times`.
fn update<'i>(path: &'i [ResourceIndex], sample_times: Option<Series<'i, Second>>) -> Event<'i> {
    Event {
        seq: UVlq32(0),
        result: Ok(EventKind::PropertyUpdate {
            path: RefVec::Slice { slice: path },
            t: None,
            missed: None,
            sample_times,
            data: TailBytes(&[1, 0]),
        }),
    }
}

#[inline(never)]
fn rewrite<'b>(
    prefix: &mut MountPrefix<PathArray<PATH_LEN_MAX>>,
    event: &[u8],
    buf: &'b mut [u8],
) -> Result<&'b [u8], RewriteError> {
    prefix.event_out(event, buf)
}

/// Decodes an `Event`, kept whole by `black_box`; true when it decoded.
#[inline(never)]
fn decode(event: &[u8]) -> bool {
    let event = Event::from_ww_bytes(event);
    let ok = event.is_ok();
    core::hint::black_box(&event);
    ok
}

/// Paints [PAINT_BYTES] below the caller's stack pointer and returns that stack pointer.
#[inline(never)]
fn paint() -> usize {
    let top = cortex_m::register::msp::read() as usize;
    // this function's own frame sits right below `top`: left alone
    for addr in (top - PAINT_BYTES..top - 64).step_by(4) {
        // SAFETY: free stack below the live frames (64 KB of RAM, the bench's frames take a few KB), written
        // before the measured call uses it
        unsafe { (addr as *mut u32).write_volatile(PAINT) }
    }
    top
}

/// Bytes below `top` written since [paint]: the deepest stack of the calls in between.
#[inline(never)]
fn high_water(top: usize) -> usize {
    let mut addr = top - PAINT_BYTES;
    // SAFETY: the same painted range as in `paint`
    while addr < top && unsafe { (addr as *const u32).read_volatile() } == PAINT {
        addr += 4;
    }
    top - addr
}

#[inline(never)]
fn mark_start() {
    // SAFETY: a `nop` touches no memory, stack or flags
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    // SAFETY: two `nop`s touch no memory, stack or flags
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}
