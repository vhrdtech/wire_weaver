//! What a `ww_stream::batch::Batcher` costs on a Cortex-M3 (API-41), through its `Producer` and `Consumer` halves:
//! a `push` of one `Timed<Ampere>`, the push that seals a full batch (the hand-off to the consumer), a `poll` that
//! seals nothing, a `poll` that seals a due batch, and `encode_into` of a full batch of 32 with the `no_std` default
//! codec next to the bare `Series::with_policy` encode of the same items (the difference is taking the store,
//! rebuilding the items into the scratch and giving the store back).
//!
//! Instructions are counted from outside, as in the `Series` encode measurement (TEST-23): [mark_start] and
//! [mark_end] bracket each measured call, and under `qemu-system-arm -accel tcg,one-insn-per-tb=on -d exec,nochain
//! -D trace.log` every retired instruction is one trace line, so a count is the line distance between a
//! `mark_start` and the next `mark_end` (addresses from `nm`), minus the first pair, which brackets nothing.
//! In order: calibration, one push, 30 pushes, the 32nd push (seals), an idle poll, `encode_into`, the bare
//! encode, a poll that seals a due batch.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::{Ampere, Prefix, Second, Timed};
use ww_stream::batch::{Batcher, DEFAULT_POLICY, Scratch, scratch_bytes};

type Sample = Timed<Ampere>;

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// A current reversing after a speed step: a ramp with some ripple, 40 µs apart.
fn sample(k: u32) -> Sample {
    let ripple = [0, 3, 5, 3, 0, -3, -5, -3][k as usize % 8];
    Timed::new(
        Second::micro(1_000_000 + k * 40),
        Ampere::new(Prefix::Milli, 2_400 - k as i32 * 150 + ripple),
    )
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let mut batcher = Batcher::<Sample, 32>::with_budget(DEFAULT_POLICY, 1024);
    let mut scratch = Scratch::<{ scratch_bytes::<Sample>(32) }>::new();
    let items: [Sample; 32] = core::array::from_fn(|k| sample(k as u32));
    let mut buf = [0u8; 512];
    let (mut producer, mut consumer) = batcher.split();

    mark_start();
    mark_end();

    mark_start();
    let first = producer.push(&items[0]);
    mark_end();

    mark_start();
    for item in &items[1..31] {
        producer.push(item);
    }
    mark_end();

    mark_start();
    let last = producer.push(&items[31]);
    mark_end();

    // a second batch filling, nothing to seal
    producer.push(&items[0]);
    mark_start();
    let idle = consumer.poll(0, Some(10_000), 1024);
    mark_end();

    mark_start();
    let enc = consumer.encode_into(&mut scratch, &mut buf);
    mark_end();

    let series = Series::with_policy(&items[..], DEFAULT_POLICY);
    mark_start();
    let bare = series.to_ww_bytes(&mut buf).map(|b| b.len());
    mark_end();

    mark_start();
    let due = consumer.poll(20_000, Some(10_000), 1024);
    mark_end();

    writeln!(stdout, "push {first:?}, 32nd {last:?}, idle poll {idle:?}, due poll {due:?}").unwrap();
    writeln!(stdout, "encode_into {enc:?}, bare {bare:?}").unwrap();
    writeln!(
        stdout,
        "batcher {} B, scratch {} B",
        core::mem::size_of::<Batcher<Sample, 32>>(),
        core::mem::size_of_val(&scratch)
    )
    .unwrap();

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
