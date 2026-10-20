//! API of the batched stream tests (API-41): plain, counted and reliable streams whose items go in batches, one
//! `Series` per event, timed and untimed, and methods that make the device produce.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
trait StreamsBatched {
    /// Plain batched stream: batches while a host has it open, nothing counted
    #[batched]
    stream!(plain: Sample);

    /// Counted batched stream; unit: sample
    #[counted]
    #[batched]
    stream!(counted: Sample);

    /// Reliable batched stream with an argument and a result; unit: sample
    #[stream(Sample, reliable, batched)]
    fn reliable(gain: i32) -> Result<(), SampleError>;

    /// Counted, untimed: a frame has its own number; unit: frame
    #[stream(Frame, counted, batched(untimed))]
    fn frames();

    /// Device pushes `count` samples into each timed stream (values counting on, `t` 40 µs apart from `t0_us`) and
    /// `count` frames
    fn produce(count: u32, t0_us: u32);
    /// Device seals the filling batches now
    fn flush();
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    series,
    final_structure,
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Sample {
    pub value: i32,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    series,
    final_structure,
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Frame {
    pub number: u32,
    pub ok: bool,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    ww_repr = u4
)]
pub enum SampleError {
    /// Gain 0.
    ZeroGain,
}

/// The batched streams in an optional trait (API-54): a server leaving it out has no polled streams to send for.
#[ww_trait]
trait BatchedBoard {
    fn ping() -> u8;
    #[optional]
    ww_impl!(streams: StreamsBatched);
}
