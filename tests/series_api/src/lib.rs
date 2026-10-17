//! Compressed sequences (`Delta`, `DeltaOfDelta`, `XorFloat`, SW-31, SW-32) in every position of an API (SW-50), and
//! `Series<T>` of structs, enums and a generic `ww_si::Timed<T>` (SW-60, CLT-29).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::{Timed, Volt};

#[ww_trait]
trait Sequences {
    /// Sum of the timestamps
    fn sum(timestamps: DeltaOfDelta<'i, u32>) -> u64;
    /// `count` values counting down from `start`
    fn ramp(start: i16, count: u8) -> Delta<'i, i16>;
    /// The values times `k`
    fn scale(values: XorFloat<'i, f32>, k: f32) -> XorFloat<'i, f32>;
    /// A struct with sequences as fields, returns the number of timestamps
    fn capture(capture: Capture<'i>) -> u32;
    property!(rw stamps: DeltaOfDelta<'i, u64>);
    stream!(samples: Delta<'i, u16>);
    /// The last `n` readings, column by column
    fn readings(n: u8) -> Series<'i, Reading<'i>>;
    /// Voltage history with device time
    fn history() -> Series<'i, Timed<Volt>>;
    stream!(batches: Series<'i, Reading<'i>>);
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    series,
    self_describing,
    ww_repr = unib32,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub enum Scale {
    Milli,
    Micro,
}

/// Every kind of column: an enum and a number restarting after it, a string run, an opaque field, an appended
/// evolvable field, an enum with data.
#[derive_shrink_wrap(owned(feature = "std"), series, derive(Debug, PartialEq, Clone))]
pub struct Reading<'i> {
    pub scale: Scale,
    pub value: i32,
    pub label: &'i str,
    #[series(opaque)]
    pub notes: RefVec<'i, &'i str>,
    pub status: Status,
    #[default = None]
    pub ok: Option<bool>,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    series,
    ww_repr = unib32,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub enum Status {
    Idle,
    Busy(u8),
    Fault { code: u16 },
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Capture<'i> {
    pub channel: u8,
    pub timestamps: DeltaOfDelta<'i, u32>,
    pub levels: XorFloat<'i, f64>,
}
