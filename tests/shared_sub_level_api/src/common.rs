//! What both lines share, unchanged.

use wire_weaver::prelude::*;

/// A raw sensor reading.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Code(pub u16);

/// A channel of the sensor: on the root in line 1, under `probe` and as the `aux[]` array in line 2.
#[ww_trait]
pub trait Channel {
    fn read() -> Code;
    /// Added on both lines, in a different revision of each.
    #[since = "w1.1, w2.1"]
    fn reset();
}
