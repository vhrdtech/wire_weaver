//! Encoded sizes of synthetic time-series data: raw `Vec<T>` vs `Delta`/`DeltaOfDelta` (integers) and `XorFloat`
//! (floats) (SW-31, SW-32). Referenced from `docs/serdes/shrink_wrap.md`.
//!
//! Run with `cargo run --example time_series`.

use shrink_wrap::prelude::*;
use shrink_wrap::series::{DeltaInt, XorBits};

const N: usize = 60; // one minute at 1 Hz

fn ser_len<S: SerializeShrinkWrap>(value: &S) -> usize {
    let mut buf = vec![0u8; 1 << 16];
    value.to_ww_bytes(&mut buf).unwrap().len()
}

fn pct(bytes: usize, raw: usize) -> f64 {
    bytes as f64 * 100.0 / raw as f64
}

/// A cheap deterministic PRNG so the example needs no extra dependency.
fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

fn report_int<T: DeltaInt + Clone + SerializeShrinkWrap + for<'i> DeserializeShrinkWrap<'i>>(
    name: &str,
    values: &[T],
) {
    let raw = ser_len(&RefVec::Slice { slice: values });
    let delta = ser_len(&Delta::new(values));
    let dod = ser_len(&DeltaOfDelta::new(values));
    println!(
        "{name:<34} raw {raw:>5} B   Delta {delta:>5} B ({:>5.1}%)   DeltaOfDelta {dod:>5} B ({:>5.1}%)",
        pct(delta, raw),
        pct(dod, raw),
    );
}

fn report_float<T: XorBits + Clone + SerializeShrinkWrap + for<'i> DeserializeShrinkWrap<'i>>(
    name: &str,
    values: &[T],
) {
    let raw = ser_len(&RefVec::Slice { slice: values });
    let xor = ser_len(&XorFloat::new(values));
    println!(
        "{name:<34} raw {raw:>5} B   XorFloat {xor:>5} B ({:>5.1}%)",
        pct(xor, raw),
    );
}

fn main() {
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;

    // monotonic timestamps, 1 s apart, with 0 or 1 s of jitter (network/scheduling noise)
    let stamps: Vec<u64> = (0..N)
        .map(|i| 1_700_000_000u64 + i as u64 + (xorshift(&mut rng) % 2))
        .collect();

    // a counter that mostly ticks by 1, with the occasional small jump (retried/coalesced packets)
    let mut count: i32 = 1_000;
    let counts: Vec<i32> = (0..N)
        .map(|_| {
            count += 1 + (xorshift(&mut rng) % 3) as i32;
            count
        })
        .collect();

    // a sensor reading: a sine wave rounded to one decimal, like a real ADC with few significant digits
    let temps: Vec<f32> = (0..N)
        .map(|i| {
            let t = i as f32 / N as f32 * core::f32::consts::TAU;
            ((21.5 + 0.5 * t.sin()) * 10.0).round() / 10.0
        })
        .collect();

    // a constant reading (a voltage rail, a fixed config value): every element is the same
    let rail: Vec<f32> = vec![3.3f32; N];

    println!("{N} samples each, one minute at 1 Hz\n");
    report_int("stamps: u64, 1s step + 0/1s jitter", &stamps);
    report_int("counts: i32, +1..=3 ticks", &counts);
    report_float("temps: f32, sine, 0.1 precision", &temps);
    report_float("rail: f32, constant", &rail);
}
