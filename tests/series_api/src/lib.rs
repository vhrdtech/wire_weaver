//! Compressed sequences (`Delta`, `DeltaOfDelta`, `XorFloat`, SW-31, SW-32) in every position of an API (SW-50).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
trait Series {
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
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Capture<'i> {
    pub channel: u8,
    pub timestamps: DeltaOfDelta<'i, u32>,
    pub levels: XorFloat<'i, f64>,
}
