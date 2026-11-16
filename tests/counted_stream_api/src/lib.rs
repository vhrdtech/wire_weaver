//! Root for the counted-stream dispatch bench (`examples_mcu/mcu_qemu` `qemu-counted-stream`, R10): a 2-level
//! indexed resource (`group[]` -> `item[]`), each item with one counted event stream, mirroring the shape (one
//! array under another, one stream at the leaf) `ww_gpio::Bank`/`Pin` had before this bench moved onto a
//! framework-only fixture.
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait CountedStreamBench {
    ww_impl!(group[]: Group);
}

#[ww_trait]
pub trait Group {
    ww_impl!(item[]: Item);
}

#[ww_trait]
pub trait Item {
    stream!(event: Edge);
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy),
    ww_repr = u1
)]
pub enum Edge {
    Rising,
    Falling,
}
