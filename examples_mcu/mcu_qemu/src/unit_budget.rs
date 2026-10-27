//! Shared body of the units firmware budget (TEST-27): what `ww_si::Fixed` costs on an MCU against the plain
//! integer fields it replaces (`shared_us: u64`, `rate_hz: u32`, `duty: u16`). Every case has a plain twin and a
//! `Fixed` twin that writes and reads the same bytes (the hash of each pair must match), so any difference is the
//! type, not the wire.
//!
//! Cases: a struct of three fields encoded and decoded (16 messages per run), a `Series` column of 32 timestamps
//! encoded and decoded (`DeltaPacked`), and construction checked against a range (64 values: `try_new` of a
//! `Fixed<_, _, _, Range<..>>` against a hand-written `if` on a plain `u32`). Later pieces (Point, rounding,
//! Level) add their cases to [Case] and to the match in [run].
//!
//! Each function under test is `#[inline(never)]` and named `plain_*` / `fixed_*`, so `llvm-nm --size-sort` of the
//! ELF gives its flash and `-Z emit-stack-sizes` its stack frame (`just units-budget`). Used by `qemu-unit-budget`
//! (instructions, `qemu_icount`) and `stm32h725ig-unit-budget-cycles` (DWT cycles), through [Meter].

// each bin uses a part
#![allow(dead_code)]

use core::hint::black_box;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use wire_weaver::shrink_wrap::series::DeltaPacked;
use ww_si::units::{Hertz, Ratio, Second};
use ww_si::{Fixed, Range, prefix};

/// Runs of each case: the first is cold, the others warm.
pub const RUNS: usize = 3;
/// Messages of the struct cases, values of the bounds cases, items of the `Series` cases.
const MSGS: usize = 16;
const VALUES: usize = 64;
const ITEMS: usize = 32;

type Micros64 = Fixed<Second, prefix::Micro, u64>;
type Micros32 = Fixed<Second, prefix::Micro, u32>;
type Hz32 = Fixed<Hertz, prefix::One, u32>;
type Duty16 = Fixed<Ratio, prefix::One, u16>;
/// A value of 0..=1000 (per mille).
type Mille = Fixed<Ratio, prefix::One, u32, Range<0, 1000>>;

#[derive_shrink_wrap(borrowed, derive(Clone, Copy))]
struct PlainStatus {
    shared_us: u64,
    rate_hz: u32,
    duty: u16,
}

#[derive_shrink_wrap(borrowed, derive(Clone, Copy))]
struct FixedStatus {
    shared: Micros64,
    rate: Hz32,
    duty: Duty16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    EncodePlain,
    EncodeFixed,
    DecodePlain,
    DecodeFixed,
    SeriesEncodePlain,
    SeriesEncodeFixed,
    SeriesDecodePlain,
    SeriesDecodeFixed,
    CheckPlain,
    CheckFixed,
}

impl Case {
    pub const ALL: [Case; 10] = [
        Case::EncodePlain,
        Case::EncodeFixed,
        Case::DecodePlain,
        Case::DecodeFixed,
        Case::SeriesEncodePlain,
        Case::SeriesEncodeFixed,
        Case::SeriesDecodePlain,
        Case::SeriesDecodeFixed,
        Case::CheckPlain,
        Case::CheckFixed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Case::EncodePlain => "struct encode: plain",
            Case::EncodeFixed => "struct encode: Fixed",
            Case::DecodePlain => "struct decode: plain",
            Case::DecodeFixed => "struct decode: Fixed",
            Case::SeriesEncodePlain => "Series encode: plain",
            Case::SeriesEncodeFixed => "Series encode: Fixed",
            Case::SeriesDecodePlain => "Series decode: plain",
            Case::SeriesDecodeFixed => "Series decode: Fixed",
            Case::CheckPlain => "range check: plain",
            Case::CheckFixed => "range check: Fixed",
        }
    }

    /// The case this one is measured against (`None` for the plain twins).
    pub fn baseline(self) -> Option<Case> {
        match self {
            Case::EncodeFixed => Some(Case::EncodePlain),
            Case::DecodeFixed => Some(Case::DecodePlain),
            Case::SeriesEncodeFixed => Some(Case::SeriesEncodePlain),
            Case::SeriesDecodeFixed => Some(Case::SeriesDecodePlain),
            Case::CheckFixed => Some(Case::CheckPlain),
            _ => None,
        }
    }

    /// The `Fixed` twin of a pair (the case with a baseline).
    pub fn is_fixed(self) -> bool {
        self.baseline().is_some()
    }

    /// Whether the `only-plain` / `only-fixed` features of this build leave the case out.
    fn skipped(self) -> bool {
        (cfg!(feature = "only-plain") && self.is_fixed())
            || (cfg!(feature = "only-fixed") && !self.is_fixed())
    }

    /// Operations per run, to give a cost per operation.
    pub fn ops(self) -> usize {
        match self {
            Case::EncodePlain | Case::EncodeFixed | Case::DecodePlain | Case::DecodeFixed => MSGS,
            Case::SeriesEncodePlain
            | Case::SeriesEncodeFixed
            | Case::SeriesDecodePlain
            | Case::SeriesDecodeFixed => ITEMS,
            Case::CheckPlain | Case::CheckFixed => VALUES,
        }
    }
}

pub trait Meter {
    /// Runs `work` once and measures it.
    fn measure(&mut self, case: Case, run: usize, work: &mut dyn FnMut());
}

/// What a case wrote and read, the same on every core and for a plain/Fixed pair when the types are right.
#[derive(Clone, Copy)]
pub struct Report {
    pub bytes: usize,
    pub hash: u32,
}

fn fnv(hash: &mut u32, bytes: &[u8]) {
    for &b in bytes {
        *hash = (*hash ^ b as u32).wrapping_mul(0x0100_0193);
    }
}

/// A fixed pseudo-random sequence (xorshift32).
fn sequence<const N: usize>(seed: u32) -> [u32; N] {
    let mut x = seed;
    core::array::from_fn(|_| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    })
}

/// Timestamps in µs as a sampled signal has them: a start and a roughly constant step.
fn times() -> [u32; ITEMS] {
    let steps = sequence::<ITEMS>(0x7157);
    let mut t = 1_730_000_000u32;
    core::array::from_fn(|i| {
        t = t.wrapping_add(40 + steps[i] % 3);
        t
    })
}

pub fn run(meter: &mut dyn Meter) -> [Report; Case::ALL.len()] {
    let mut reports = [Report {
        bytes: 0,
        hash: 0x811c_9dc5,
    }; Case::ALL.len()];
    let seeds = black_box(sequence::<MSGS>(0xACE1));
    let times = black_box(times());
    // the column as a firmware holds it: the same numbers, as Fixed (built outside the measured call)
    #[cfg(not(feature = "only-plain"))]
    let fixed_times = black_box(times.map(Micros32::new));
    // out of range now and then, so the check has both outcomes
    let values = black_box(sequence::<VALUES>(0xBEEF).map(|x| x % 1400));

    // the bytes the read cases read, written once
    let mut status_buf = [0u8; 256];
    // a build with only one twin makes the input with that twin
    #[cfg(not(feature = "only-fixed"))]
    let status_len = plain_encode(&mut status_buf, &seeds);
    #[cfg(feature = "only-fixed")]
    let status_len = fixed_encode(&mut status_buf, &seeds);
    let mut series_buf = [0u8; 256];
    #[cfg(not(feature = "only-fixed"))]
    let series_len = plain_series_encode(&mut series_buf, &times);
    #[cfg(feature = "only-fixed")]
    let series_len = fixed_series_encode(&mut series_buf, &fixed_times);

    for case in Case::ALL {
        if case.skipped() {
            continue;
        }
        let report = &mut reports[case as usize];
        for run in 0..RUNS {
            let mut buf = [0u8; 256];
            let mut len = 0;
            let mut sum = 0u32;
            let mut work = || match case {
                #[cfg(not(feature = "only-fixed"))]
                Case::EncodePlain => len = plain_encode(&mut buf, &seeds),
                #[cfg(not(feature = "only-plain"))]
                Case::EncodeFixed => len = fixed_encode(&mut buf, &seeds),
                #[cfg(not(feature = "only-fixed"))]
                Case::DecodePlain => sum = plain_decode(&status_buf[..status_len]),
                #[cfg(not(feature = "only-plain"))]
                Case::DecodeFixed => sum = fixed_decode(&status_buf[..status_len]),
                #[cfg(not(feature = "only-fixed"))]
                Case::SeriesEncodePlain => len = plain_series_encode(&mut buf, &times),
                #[cfg(not(feature = "only-plain"))]
                Case::SeriesEncodeFixed => len = fixed_series_encode(&mut buf, &fixed_times),
                #[cfg(not(feature = "only-fixed"))]
                Case::SeriesDecodePlain => sum = plain_series_decode(&series_buf[..series_len]),
                #[cfg(not(feature = "only-plain"))]
                Case::SeriesDecodeFixed => sum = fixed_series_decode(&series_buf[..series_len]),
                #[cfg(not(feature = "only-fixed"))]
                Case::CheckPlain => sum = plain_check(&values),
                #[cfg(not(feature = "only-plain"))]
                Case::CheckFixed => sum = fixed_check(&values),
                #[allow(unreachable_patterns)]
                _ => {}
            };
            meter.measure(case, run, &mut work);
            if run == 0 {
                report.bytes = len;
                fnv(&mut report.hash, &buf[..len]);
                fnv(&mut report.hash, &sum.to_le_bytes());
            }
        }
    }
    reports
}

fn status_values(seed: u32) -> (u64, u32, u16) {
    (
        1_730_000_000_000_000 + (seed as u64) * 40,
        seed >> 8,
        (seed >> 16) as u16,
    )
}

#[inline(never)]
fn plain_encode(buf: &mut [u8], seeds: &[u32]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &seed in seeds {
        let (shared_us, rate_hz, duty) = status_values(seed);
        PlainStatus {
            shared_us,
            rate_hz,
            duty,
        }
        .ser_shrink_wrap(&mut wr)
        .unwrap();
    }
    wr.finish().unwrap().len()
}

#[inline(never)]
fn fixed_encode(buf: &mut [u8], seeds: &[u32]) -> usize {
    let mut wr = BufWriter::new(buf);
    for &seed in seeds {
        let (shared_us, rate_hz, duty) = status_values(seed);
        FixedStatus {
            shared: Micros64::new(shared_us),
            rate: Hz32::new(rate_hz),
            duty: Duty16::new(duty),
        }
        .ser_shrink_wrap(&mut wr)
        .unwrap();
    }
    wr.finish().unwrap().len()
}

#[inline(never)]
fn plain_decode(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for _ in 0..MSGS {
        let s = PlainStatus::des_shrink_wrap(&mut rd).unwrap();
        sum = sum.rotate_left(5)
            ^ s.shared_us as u32
            ^ (s.shared_us >> 32) as u32
            ^ s.rate_hz
            ^ s.duty as u32;
    }
    sum
}

#[inline(never)]
fn fixed_decode(buf: &[u8]) -> u32 {
    let mut rd = BufReader::new(buf);
    let mut sum = 0u32;
    for _ in 0..MSGS {
        let s = FixedStatus::des_shrink_wrap(&mut rd).unwrap();
        let us = s.shared.get();
        sum =
            sum.rotate_left(5) ^ us as u32 ^ (us >> 32) as u32 ^ s.rate.get() ^ s.duty.get() as u32;
    }
    sum
}

#[inline(never)]
fn plain_series_encode(buf: &mut [u8], times: &[u32]) -> usize {
    Series::with_policy(times, DeltaPacked)
        .to_ww_bytes(buf)
        .unwrap()
        .len()
}

#[inline(never)]
fn fixed_series_encode(buf: &mut [u8], items: &[Micros32]) -> usize {
    Series::with_policy(items, DeltaPacked)
        .to_ww_bytes(buf)
        .unwrap()
        .len()
}

#[inline(never)]
fn plain_series_decode(buf: &[u8]) -> u32 {
    let series = Series::<u32>::from_ww_bytes(buf).unwrap();
    let mut sum = 0u32;
    for t in series.iter() {
        sum = sum.rotate_left(3) ^ t;
    }
    sum
}

#[inline(never)]
fn fixed_series_decode(buf: &[u8]) -> u32 {
    let series = Series::<Micros32>::from_ww_bytes(buf).unwrap();
    let mut sum = 0u32;
    for t in series.iter() {
        sum = sum.rotate_left(3) ^ t.get();
    }
    sum
}

/// Per mille values accepted, summed: what a setter does with a value from outside.
#[inline(never)]
fn plain_check(values: &[u32]) -> u32 {
    let mut sum = 0u32;
    for &v in values {
        let checked = if v <= 1000 { Some(v) } else { None };
        sum = sum.rotate_left(3) ^ checked.map_or(0xFFFF_FFFF, |v| v + 1);
    }
    sum
}

#[inline(never)]
fn fixed_check(values: &[u32]) -> u32 {
    let mut sum = 0u32;
    for &v in values {
        let checked = Mille::try_new(v).ok();
        sum = sum.rotate_left(3) ^ checked.map_or(0xFFFF_FFFF, |v| v.get() + 1);
    }
    sum
}
