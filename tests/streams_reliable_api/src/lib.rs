//! API of the counted and reliable stream tests (API-30, API-31): stream functions with arguments and a result,
//! `#[counted] stream!`, and methods that make the device produce.
use wire_weaver::prelude::*;

#[ww_trait]
trait StreamsReliable {
    /// Bytes received on a line, as a counted stream; unit: byte
    #[counted]
    stream!(rx: [u8]);

    /// Samples at `config.rate`, kept in the device's store until acknowledged; unit: sample
    #[stream(Sample, reliable)]
    fn samples(config: SampleConfig) -> Result<SampleActual, SampleError>;

    /// Counted stream with an argument: `chunk` bytes per event; unit: byte
    #[stream(Vec<u8>, counted)]
    fn chunks(chunk: u8);

    /// Device sends `count` events of `len` bytes on `rx`
    fn emit_rx(len: u8, count: u8);
    /// The producer of `rx` dropped `units` bytes
    fn lose_rx(units: u32);
    /// Device pushes `count` samples into the store of `samples`, values counting on from the last one
    fn produce(count: u32);
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct Sample {
    pub value: u32,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct SampleConfig {
    pub rate: u32,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy)
)]
pub struct SampleActual {
    pub rate: u32,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    ww_repr = u4
)]
pub enum SampleError {
    BadRate,
}
