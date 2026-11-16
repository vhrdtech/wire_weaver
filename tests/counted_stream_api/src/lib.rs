//! Root for the counted-stream dispatch bench (`examples_mcu/mcu_qemu` `qemu-counted-stream`, R10): a 2-level
//! indexed resource (`group[]` -> `item[]`), each item with one counted event stream, mirroring the shape (one
//! array under another, one stream at the leaf) `ww_gpio::Bank`/`Pin` had before this bench moved onto a
//! framework-only fixture.
#![no_std]

use ww_api::prelude::*;

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
    /// Edges of this item as they happen: opening the stream starts them, closing it stops them. A counted stream,
    /// as `ww_gpio::Pin::event` is, so the host learns exactly how many events the device dropped.
    #[stream(Edge, counted)]
    fn event();
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
