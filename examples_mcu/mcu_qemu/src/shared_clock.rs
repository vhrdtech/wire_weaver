//! Shared body of the shared clock benches (idea I-2): one batch of `N` samples of all 21 FOC properties of the
//! motor demo ([super::foc_signals]) encoded the way batched observe sends them today, 21 x `Series<Timed<T>>`
//! (each property its own time column), and the way a shared clock would, 21 x `Series<T>` plus one
//! `Series<Second>` for the level, and as one `Series<Timed<FocRow>>` with an `Option` column per property (the
//! shape the shared clock didn't take: a struct series restarts every column after a changed tag, quadratic over
//! 21 tagged fields, 7x the cycles). Used by the `qemu-shared-clock-*` bins (instructions, counted from outside with
//! `qemu_icount`) and by the H725 bin `stm32h725ig-shared-clock-cycles` (DWT cycles), through [Meter].
//!
//! Each property is built and measured on its own (one [Meter::measure] per `Series`): a whole level of items at
//! once is 16 KB of `Timed<T>` and more than the Cortex-M3 model's 64 KB of RAM once the compiler keeps copies on
//! the stack.

// each bin uses a part: `run` or `run_store`
#![allow(dead_code)]

use super::foc_signals as sig;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::series::{SeriesPolicy, SeriesStore, write_store_series};
use wire_weaver::shrink_wrap::{BufWriter, Series};
use ww_si::{Ampere, Radian, Ratio, Second, Timed, Volt, Weber};

/// Samples per property per batch.
pub const N: usize = 32;
/// Batches in the recorded signals.
pub const BATCHES: usize = 2;

/// What a measured `Series` is; the `Store*` ones are the shared clock's `Series` written straight from a
/// `SeriesStore` of the items (`write_store_series`, the column-by-column writer, experiment series-memcpy-fast),
/// the others by the item walk.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// One property's `Series<Timed<T>>`: what batched observe sends today.
    Timed,
    /// One property's `Series<T>`, untimed: a shared clock.
    Untimed,
    /// The level's one `Series<Second>`.
    Clock,
    /// One `Series<Timed<FocRow>>`, every property an `Option` column next to one time column: a shared clock as
    /// one series per level batch.
    Row,
    StoreUntimed,
    StoreClock,
}

impl Group {
    /// What [run] measures.
    pub const ALL: [Group; 4] = [Group::Timed, Group::Untimed, Group::Clock, Group::Row];
    /// What [run_store] measures.
    pub const STORE: [Group; 4] = [
        Group::Untimed,
        Group::Clock,
        Group::StoreUntimed,
        Group::StoreClock,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Timed => "21 x Series<Timed<T>>",
            Group::Untimed => "21 x Series<T>",
            Group::Clock => "1 x Series<Second>",
            Group::Row => "1 x Series<Timed<Row of 21 Option<T>>>",
            Group::StoreUntimed => "21 x Series<T> from the store",
            Group::StoreClock => "1 x Series<Second> from the store",
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

/// Builds one `Series`' items and measures their encode by the item walk, in a frame of its own (the items of one
/// property, not of the whole level, on the stack at a time).
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
    let mut buf = [0u8; 4096];
    meter.measure(group, batch, &mut || {
        Series::with_policy(&items[..], policy)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .len()
    });
}

/// [one], then the same `Series` written from a `SeriesStore` of the items as `store_group`: the same bytes.
#[inline(never)]
fn one_and_store<T, P: SeriesPolicy + Copy>(
    group: Group,
    store_group: Group,
    batch: usize,
    item: impl Fn(usize) -> T,
    policy: P,
    meter: &mut impl Meter,
) where
    T: for<'i> wire_weaver::shrink_wrap::series::SeriesColumns<'i> + SeriesStore,
{
    let items: [T; N] = core::array::from_fn(item);
    walk_and_store(group, store_group, batch, &items, policy, meter);
}

/// The body of [one_and_store], once per item type (not per property: the flash).
#[inline(never)]
fn walk_and_store<T, P: SeriesPolicy + Copy>(
    group: Group,
    store_group: Group,
    batch: usize,
    items: &[T; N],
    policy: P,
    meter: &mut impl Meter,
) where
    T: for<'i> wire_weaver::shrink_wrap::series::SeriesColumns<'i> + SeriesStore,
{
    let mut buf = [0u8; 4096];
    let mut walk = 0;
    meter.measure(group, batch, &mut || {
        walk = Series::with_policy(&items[..], policy)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .len();
        walk
    });
    let mut store = T::empty_store::<N>();
    for (i, item) in items.iter().enumerate() {
        T::set(&mut store, i, item);
    }
    let mut from_store = [0u8; 4096];
    meter.measure(store_group, batch, &mut || {
        let mut wr = BufWriter::new(&mut from_store);
        assert!(write_store_series::<T, P, N>(&mut wr, &store, 0..N, &policy).unwrap());
        wr.finish_and_take().unwrap().len()
    });
    assert_eq!(buf[..walk], from_store[..walk], "store and walk differ");
}

/// One sample of the level, every property optional (observed or not, on its grid or not).
#[derive_shrink_wrap(borrowed, derive(Copy, Clone), final_structure, series)]
pub struct FocRow {
    pub i_a: Option<Ampere>,
    pub i_b: Option<Ampere>,
    pub i_c: Option<Ampere>,
    pub i_alpha: Option<Ampere>,
    pub i_beta: Option<Ampere>,
    pub i_d: Option<Ampere>,
    pub i_q: Option<Ampere>,
    pub v_d: Option<Volt>,
    pub v_q: Option<Volt>,
    pub v_alpha: Option<Volt>,
    pub v_beta: Option<Volt>,
    pub duty_a: Option<Ratio>,
    pub duty_b: Option<Ratio>,
    pub duty_c: Option<Ratio>,
    pub phase: Option<Radian>,
    pub phase_observer: Option<Radian>,
    pub phase_encoder: Option<Radian>,
    pub observer_x1: Option<Weber>,
    pub observer_x2: Option<Weber>,
    pub v_bus: Option<Volt>,
    pub phase_true: Option<Radian>,
}

macro_rules! level {
    ($($name:ident: $ty:ident = $prefix:ident($signal:ident),)*) => {

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
                one(Group::Row, batch, |i| {
                    Timed::new(t(batch, i), FocRow {
                        $($name: Some($ty::$prefix(sig::$signal[batch * N + i])),)*
                    })
                }, policy, meter);
            }
        }

        /// Per batch: the 21 untimed series and the clock, each by the item walk and then from a store (in
        /// separate bins from [run]: the Cortex-M3 model's 256 KB of flash don't hold both).
        pub fn run_store<P: SeriesPolicy + Copy>(policy: P, meter: &mut impl Meter) {
            for batch in 0..BATCHES {
                $(
                    one_and_store(
                        Group::Untimed,
                        Group::StoreUntimed,
                        batch,
                        |i| $ty::$prefix(sig::$signal[batch * N + i]),
                        policy,
                        meter,
                    );
                )*
                one_and_store(Group::Clock, Group::StoreClock, batch, |i| t(batch, i), policy, meter);
            }
        }
    };
}

level! {
    i_a: Ampere = milli(I_A),
    i_b: Ampere = milli(I_B),
    i_c: Ampere = milli(I_C),
    i_alpha: Ampere = milli(I_ALPHA),
    i_beta: Ampere = milli(I_BETA),
    i_d: Ampere = milli(I_D),
    i_q: Ampere = milli(I_Q),
    v_d: Volt = milli(V_D),
    v_q: Volt = milli(V_Q),
    v_alpha: Volt = milli(V_ALPHA),
    v_beta: Volt = milli(V_BETA),
    duty_a: Ratio = micro(DUTY_A),
    duty_b: Ratio = micro(DUTY_B),
    duty_c: Ratio = micro(DUTY_C),
    phase: Radian = micro(PHASE),
    phase_observer: Radian = micro(PHASE_OBSERVER),
    phase_encoder: Radian = micro(PHASE_ENCODER),
    observer_x1: Weber = nano(OBSERVER_X1),
    observer_x2: Weber = nano(OBSERVER_X2),
    v_bus: Volt = milli(V_BUS),
    phase_true: Radian = micro(PHASE_TRUE),
}
