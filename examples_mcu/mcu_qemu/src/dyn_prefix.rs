//! Shared body of the dynamic-prefix benches: what a prefix carried per item costs in a `Series`. One FOC level batch
//! of 32 samples of all 21 motor demo properties ([super::foc_signals]), two batches, written three ways:
//!
//! - **quantity**: each property a `Series<Quantity<U>>` (`Ampere`, `Volt`, ..), the API's prefix (mA, µ, nV) and
//!   `I32` in every item, as the motor demo's observe cells send them today, plus the level's `Series<Second>`
//!   clock;
//! - **fixed**: the same numbers as `Series<Fixed<U, Milli, i32>>` (the prefix and number type in the type, not on
//!   the wire), plus a `Series<DeviceMicros<u32>>` clock;
//! - **auto**: each property a `Quantity` that autoranges: the finest prefix from the API's up whose number fits an
//!   `i16`, rounded to the nearest and still stored as `I32`, so only the prefix changes, mid-batch where a signal
//!   crosses a range (an angle that wraps, a flux through zero, a current ramp).
//!
//! The quantity and fixed columns are measured by the item walk (`Series::with_policy` over the items) and from a
//! `SeriesStore` (`write_store_series`, what a level's columns go through on a device). The auto columns are
//! measured the way `ww_stream`'s `Scratch::encode_column` writes them: from the store when the batch keeps one
//! prefix, else the store refuses, the items are rebuilt from it and walked. One bin per way
//! (`qemu-dyn-prefix-quantity`, `-fixed`, `-auto`), so each ELF's `.text` is that way's flash; the H725 bin
//! `stm32h725ig-dyn-prefix-cycles` runs all three for DWT cycles.

// each bin uses a part
#![allow(dead_code)]

use super::foc_signals as sig;
use wire_weaver::shrink_wrap::series::{
    SeriesColumns, SeriesPolicy, SeriesStore, write_store_series,
};
use wire_weaver::shrink_wrap::{BufWriter, SerializeShrinkWrap, Series};
use ww_si::origins::DeviceMicros;
use ww_si::prefix::{Micro, Milli, Nano};
use ww_si::ww_numeric::NumericValue;
use ww_si::{Ampere, Fixed, Prefix, Radian, Ratio, Second, Volt, Weber, units};

/// Samples per property per batch.
pub const N: usize = 32;
/// Batches in the recorded signals.
pub const BATCHES: usize = 2;

/// What a measured `Series` is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// The 21 `Series<Quantity<U>>`, one prefix per property, by the item walk.
    QuantityWalk,
    /// The same, from the store.
    QuantityStore,
    /// The level's `Series<Second>`, from the store.
    ClockSecond,
    /// The 21 `Series<Fixed<U, P, i32>>`, by the item walk.
    FixedWalk,
    /// The same, from the store.
    FixedStore,
    /// The level's `Series<DeviceMicros<u32>>`, from the store.
    ClockPoint,
    /// The 21 autoranged `Series<Quantity<U>>`, from the store where a batch keeps one prefix, else rebuilt and
    /// walked.
    Auto,
}

impl Group {
    pub const COUNT: usize = 7;
    pub const QUANTITY: [Group; 3] = [
        Group::QuantityWalk,
        Group::QuantityStore,
        Group::ClockSecond,
    ];
    pub const FIXED: [Group; 3] = [Group::FixedWalk, Group::FixedStore, Group::ClockPoint];
    pub const AUTO: [Group; 1] = [Group::Auto];

    pub fn label(self) -> &'static str {
        match self {
            Group::QuantityWalk => "21 x Series<Quantity> walk",
            Group::QuantityStore => "21 x Series<Quantity> from the store",
            Group::ClockSecond => "1 x Series<Second> from the store",
            Group::FixedWalk => "21 x Series<Fixed> walk",
            Group::FixedStore => "21 x Series<Fixed> from the store",
            Group::ClockPoint => "1 x Series<DeviceMicros<u32>> from the store",
            Group::Auto => "21 x Series<Quantity> autoranged, store or walk",
        }
    }
}

/// Runs one measured piece of work and reports it: `work` returns the bytes it wrote.
pub trait Meter {
    fn measure(&mut self, group: Group, batch: usize, work: &mut dyn FnMut() -> usize);
    /// Notes how many columns of a batch changed prefix mid-batch and how many changes they had, for the auto way.
    fn changes(&mut self, _batch: usize, _columns: usize, _changes: usize) {}
}

/// A series written by the item walk, measured.
#[inline(never)]
fn walk<T, P: SeriesPolicy + Copy>(
    group: Group,
    batch: usize,
    items: &[T; N],
    policy: P,
    meter: &mut impl Meter,
) where
    T: for<'i> SeriesColumns<'i>,
{
    let mut buf = [0u8; 1024];
    meter.measure(group, batch, &mut || {
        Series::with_policy(&items[..], policy)
            .to_ww_bytes(&mut buf)
            .unwrap()
            .len()
    });
}

/// A series written from a store of `items`, measured; panics when the store refuses.
#[inline(never)]
fn from_store<T, P: SeriesPolicy + Copy>(
    group: Group,
    batch: usize,
    items: &[T; N],
    policy: P,
    meter: &mut impl Meter,
) where
    T: for<'i> SeriesColumns<'i> + SeriesStore,
{
    let store = store_of(items);
    let mut buf = [0u8; 1024];
    meter.measure(group, batch, &mut || {
        let mut wr = BufWriter::new(&mut buf);
        assert!(write_store_series::<T, P, N>(&mut wr, &store, 0..N, &policy).unwrap());
        wr.finish_and_take().unwrap().len()
    });
}

/// A series written as a level column is: from the store when it can, else rebuilt from it and walked.
#[inline(never)]
fn store_or_walk<T, P: SeriesPolicy + Copy>(
    group: Group,
    batch: usize,
    items: &[T; N],
    policy: P,
    meter: &mut impl Meter,
) where
    T: for<'i> SeriesColumns<'i> + SeriesStore,
{
    let store = store_of(items);
    let mut buf = [0u8; 1024];
    meter.measure(group, batch, &mut || {
        let mut wr = BufWriter::new(&mut buf);
        if write_store_series::<T, P, N>(&mut wr, &store, 0..N, &policy).unwrap() {
            return wr.finish_and_take().unwrap().len();
        }
        let rebuilt: [T; N] = core::array::from_fn(|i| T::get::<N>(&store, i));
        Series::with_policy(&rebuilt[..], policy)
            .to_ww_bytes(wr.deinit())
            .unwrap()
            .len()
    });
}

fn store_of<T: SeriesStore>(items: &[T; N]) -> T::Store<N> {
    let mut store = T::empty_store::<N>();
    for (i, item) in items.iter().enumerate() {
        T::set(&mut store, i, item);
    }
    store
}

/// The level's clock at sample `i` of `batch`, in µs.
fn t_us(batch: usize, i: usize) -> u32 {
    sig::T0_US + (batch * N + i) as u32 * sig::DT_US
}

/// `x` in units of `prefix`, autoranged: the finest prefix from `prefix` up (steps of 1000) whose number fits an
/// `i16`, rounded to the nearest.
fn autorange(mut x: i32, mut prefix: Prefix) -> (Prefix, i16) {
    while i16::try_from(x).is_err() {
        x = (x + 500 * x.signum()) / 1000;
        prefix = match prefix {
            Prefix::Nano => Prefix::Micro,
            Prefix::Micro => Prefix::Milli,
            Prefix::Milli => Prefix::Unit,
            Prefix::Unit => Prefix::Kilo,
            _ => panic!("autorange: no prefix above"),
        };
    }
    (prefix, x as i16)
}

/// Columns of `items` whose prefix changes in the batch, and the changes.
fn prefix_changes<U: ww_si::Unit>(items: &[ww_si::Quantity<U>; N]) -> usize {
    items
        .windows(2)
        .filter(|w| w[0].prefix != w[1].prefix)
        .count()
}

macro_rules! level {
    ($($name:ident: $Q:ident, $Unit:ident, $P:ident = $prefix:ident($signal:ident),)*) => {
        /// The quantity way: per batch the 21 series by the walk, then from the store, then the clock.
        pub fn run_quantity<P: SeriesPolicy + Copy>(policy: P, meter: &mut impl Meter) {
            for batch in 0..BATCHES {
                $(
                    let items: [$Q; N] = core::array::from_fn(|i| {
                        $Q::new(Prefix::$prefix, NumericValue::I32(sig::$signal[batch * N + i]))
                    });
                    walk(Group::QuantityWalk, batch, &items, policy, meter);
                    from_store(Group::QuantityStore, batch, &items, policy, meter);
                )*
                let clock: [Second; N] = core::array::from_fn(|i| Second::micro(t_us(batch, i)));
                from_store(Group::ClockSecond, batch, &clock, policy, meter);
            }
        }

        /// The fixed way: the same numbers with the prefix and number type in the type.
        pub fn run_fixed<P: SeriesPolicy + Copy>(policy: P, meter: &mut impl Meter) {
            for batch in 0..BATCHES {
                $(
                    let items: [Fixed<units::$Unit, $P, i32>; N] =
                        core::array::from_fn(|i| Fixed::new(sig::$signal[batch * N + i]));
                    walk(Group::FixedWalk, batch, &items, policy, meter);
                    from_store(Group::FixedStore, batch, &items, policy, meter);
                )*
                let clock: [DeviceMicros<u32>; N] =
                    core::array::from_fn(|i| DeviceMicros::from_raw(t_us(batch, i)));
                from_store(Group::ClockPoint, batch, &clock, policy, meter);
            }
        }

        /// The auto way: every property autoranged to fit an `i16`; the level's clock as in the quantity way is left
        /// out (the same bytes and work).
        pub fn run_auto<P: SeriesPolicy + Copy>(policy: P, meter: &mut impl Meter) {
            for batch in 0..BATCHES {
                let (mut columns, mut changes) = (0, 0);
                $(
                    let items: [$Q; N] = core::array::from_fn(|i| {
                        let (p, x) = autorange(sig::$signal[batch * N + i], Prefix::$prefix);
                        $Q::new(p, NumericValue::I32(x as i32))
                    });
                    let c = prefix_changes(&items);
                    columns += (c > 0) as usize;
                    changes += c;
                    store_or_walk(Group::Auto, batch, &items, policy, meter);
                )*
                meter.changes(batch, columns, changes);
            }
        }
    };
}

level! {
    i_a: Ampere, Ampere, Milli = Milli(I_A),
    i_b: Ampere, Ampere, Milli = Milli(I_B),
    i_c: Ampere, Ampere, Milli = Milli(I_C),
    i_alpha: Ampere, Ampere, Milli = Milli(I_ALPHA),
    i_beta: Ampere, Ampere, Milli = Milli(I_BETA),
    i_d: Ampere, Ampere, Milli = Milli(I_D),
    i_q: Ampere, Ampere, Milli = Milli(I_Q),
    v_d: Volt, Volt, Milli = Milli(V_D),
    v_q: Volt, Volt, Milli = Milli(V_Q),
    v_alpha: Volt, Volt, Milli = Milli(V_ALPHA),
    v_beta: Volt, Volt, Milli = Milli(V_BETA),
    duty_a: Ratio, Ratio, Micro = Micro(DUTY_A),
    duty_b: Ratio, Ratio, Micro = Micro(DUTY_B),
    duty_c: Ratio, Ratio, Micro = Micro(DUTY_C),
    phase: Radian, Radian, Micro = Micro(PHASE),
    phase_observer: Radian, Radian, Micro = Micro(PHASE_OBSERVER),
    phase_encoder: Radian, Radian, Micro = Micro(PHASE_ENCODER),
    observer_x1: Weber, Weber, Nano = Nano(OBSERVER_X1),
    observer_x2: Weber, Weber, Nano = Nano(OBSERVER_X2),
    v_bus: Volt, Volt, Milli = Milli(V_BUS),
    phase_true: Radian, Radian, Micro = Micro(PHASE_TRUE),
}
