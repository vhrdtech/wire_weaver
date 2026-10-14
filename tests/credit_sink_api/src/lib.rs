use wire_weaver::prelude::*;

/// Sinks with credit-based flow control (API-28), see `docs/api/streams.md`.
#[ww_trait]
trait CreditSinks {
    /// Bulk bytes, e.g., an update plan
    #[credit]
    sink!(plan: [u8]);
    /// Typed values
    #[credit]
    sink!(points: Point);
    /// Array of credit sinks
    #[credit]
    sink!(lanes[]: [u8]);
    /// Plain sink, no flow control
    sink!(plain: [u8]);
    /// Bounded byte chunks (SW-19 MaxLen)
    #[credit]
    sink!(blocks: MaxLen<RefVec<'i, u8>, 4>);
    fn finish();
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    sized
)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}
