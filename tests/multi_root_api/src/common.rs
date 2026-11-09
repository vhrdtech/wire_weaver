//! What both lines share: one Rust type each, nothing to convert between the lines.

use wire_weaver::prelude::*;

/// A raw sensor reading, as the ADC gives it.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, Clone, Copy, PartialEq, Eq))]
pub struct Code(pub u16);

/// A channel of the sensor, the same on both lines.
#[ww_trait]
pub trait Channel {
    fn read() -> Code;
    /// Added on both lines, in a different revision of each: the root's line picks its own.
    #[since = "w1.2, w2.1"]
    fn reset();
}
