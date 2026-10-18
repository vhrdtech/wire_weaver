//! Shared body of the shared clock benches (idea I-2): one batch of `N` samples of all 21 FOC properties of the
//! motor demo ([super::foc_signals]) encoded the way batched observe sends them today, 21 x `Series<Timed<T>>`
//! (each property its own time column), and the way a shared clock would, 21 x `Series<T>` plus one
//! `Series<Second>` for the level. Used by the `qemu-shared-clock-*` bins (instructions, counted from outside with
//! `qemu_icount`) and by the H725 bin `stm32h725ig-shared-clock-cycles` (DWT cycles), through [Meter].
//!
//! Each property is built and measured on its own (one [Meter::measure] per `Series`): a whole level of items at
//! once is 16 KB of `Timed<T>` and more than the Cortex-M3 model's 64 KB of RAM once the compiler keeps copies on
//! the stack.

use super::foc_signals as sig;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use wire_weaver::shrink_wrap::series::SeriesPolicy;
use ww_si::{Ampere, Radian, Ratio, Second, Timed, Volt, Weber};

/// Samples per property per batch.
pub const N: usize = 32;
/// Batches in the recorded signals.
pub const BATCHES: usize = 2;

/// What a measured `Series` is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// One property's `Series<Timed<T>>`: what batched observe sends today.
    Timed,
    /// One property's `Series<T>`, untimed: a shared clock.
    Untimed,
    /// The level's one `Series<Second>`.
    Clock,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Timed, Group::Untimed, Group::Clock];

    pub fn label(self) -> &'static str {
        match self {
            Group::Timed => "21 x Series<Timed<T>>",
            Group::Untimed => "21 x Series<T>",
            Group::Clock => "1 x Series<Second>",
        }
    }
}

/// Runs one measured piece of work and reports it: `work` returns the bytes it wrote.
pub trait Meter {
    fn measure(&mut self, group: Group, batch: usize, work: &mut dyn FnMut() -> usize);
}

fn t(batch: usize, i: usize) -> Second {
    Second::micro(sig::T0_US + (batch * N + i) as u32 * sig::DT_US)
}

/// Builds one `Series`' items and measures their encode, in a frame of its own (the items of one property, not of
/// the whole level, on the stack at a time).
#[inline(never)]
fn one<T, P: SeriesPolicy + Copy>(
    group: Group,
    batch: usize,
    item: impl Fn(usize) -> T,
    policy: P,
    meter: &mut impl Meter,
) where
    T: for<'i> wire_weaver::shrink_wrap::series::SeriesColumns<'i>,
{
    let items: [T; N] = core::array::from_fn(item);
    let mut buf = [0u8; 512];
    meter.measure(group, batch, &mut || {
        Series::with_policy(&items[..], policy)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .len()
    });
}

macro_rules! level {
    ($($ty:ident = $prefix:ident($signal:ident),)*) => {
        /// Per batch: the 21 timed series, the 21 untimed ones, the clock.
        pub fn run<P: SeriesPolicy + Copy>(policy: P, meter: &mut impl Meter) {
            for batch in 0..BATCHES {
                $(
                    one(Group::Timed, batch, |i| {
                        Timed::new(t(batch, i), $ty::$prefix(sig::$signal[batch * N + i]))
                    }, policy, meter);
                )*
                $(
                    one(Group::Untimed, batch, |i| $ty::$prefix(sig::$signal[batch * N + i]), policy, meter);
                )*
                one(Group::Clock, batch, |i| t(batch, i), policy, meter);
            }
        }
    };
}

level! {
    Ampere = milli(I_A),
    Ampere = milli(I_B),
    Ampere = milli(I_C),
    Ampere = milli(I_ALPHA),
    Ampere = milli(I_BETA),
    Ampere = milli(I_D),
    Ampere = milli(I_Q),
    Volt = milli(V_D),
    Volt = milli(V_Q),
    Volt = milli(V_ALPHA),
    Volt = milli(V_BETA),
    Ratio = micro(DUTY_A),
    Ratio = micro(DUTY_B),
    Ratio = micro(DUTY_C),
    Radian = micro(PHASE),
    Radian = micro(PHASE_OBSERVER),
    Radian = micro(PHASE_ENCODER),
    Weber = nano(OBSERVER_X1),
    Weber = nano(OBSERVER_X2),
    Volt = milli(V_BUS),
    Radian = micro(PHASE_TRUE),
}
