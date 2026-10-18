//! API of the batched observe tests (API-39): `+observe+batched` properties a host can observe every sample of,
//! next to a plain `+observe` one on the same level, and a nested level with a sampler of its own.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
trait ObserveBatched {
    /// Every sample observable, a number
    property!(ro+observe+batched current: i32);
    /// Every sample observable, a struct
    property!(ro+observe+batched reading: Reading);
    /// Observable, not batched
    property!(rw+observe mode: u8);
    /// Device takes `count` samples of the root level, `dt_us` apart from `t0_us` (values counting on from
    /// `first`), and of `slow` every tenth
    fn sample(count: u32, t0_us: u32, dt_us: u32, first: i32);
    ww_impl!(slow: Slow);
}

#[ww_trait]
trait Slow {
    property!(ro+observe+batched temp: f32);
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    series(cells),
    final_structure,
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Reading {
    pub value: i32,
    pub ok: bool,
}
