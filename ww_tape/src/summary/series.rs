//! Series summaries: the statistics a series track declares, per channel and node, and how they merge.

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use exact_units::Magnitude;
use shrink_wrap::prelude::*;

use crate::Element;

/// How a minimum or maximum of `element` is stored.
fn kind(element: Element) -> ValueKind {
    if element.is_float() {
        ValueKind::Float
    } else if element.is_signed() {
        ValueKind::Signed
    } else {
        ValueKind::Unsigned
    }
}

/// Scalars per sample of `element`: 2 for a complex element, else 1.
pub(super) fn components(element: Element) -> u8 {
    if element.parts() == 2 { 2 } else { 1 }
}

/// Whether `element` is a complex pair.
pub(super) fn is_complex(element: Element) -> bool {
    element.parts() == 2
}

/// `v` rounded to the nearest f64: sums, means and minimum or maximum read as a float.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "rounding to the nearest f64 is the result asked for"
)]
fn u64_f64(v: u64) -> f64 {
    v as f64
}

/// `v` rounded to the nearest f64.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "rounding to the nearest f64 is the result asked for"
)]
fn i64_f64(v: i64) -> f64 {
    v as f64
}

/// `v` rounded to the nearest f64: an exact integer sum stored as the format's f64.
#[cfg(feature = "std")]
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the format stores sums as f64: rounding to the nearest is its meaning"
)]
fn i128_f64(v: i128) -> f64 {
    v as f64
}

/// `v` rounded to the nearest f32: the format stores powers as f32.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the format stores powers as f32: rounding to the nearest is its meaning"
)]
fn f64_f32(v: f64) -> f32 {
    v as f32
}

/// The statistics a series track keeps per channel and node. Each merges from two adjacent nodes without the samples.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy, Default, Hash)
)]
pub struct SeriesStats {
    /// The smallest sample (per component), NaN left out.
    pub min: bool,
    /// The largest sample (per component), NaN left out.
    pub max: bool,
    /// The sum of the samples (per component), f64; the mean is the sum over the node's samples.
    pub sum: bool,
    /// The sum of the squares (per component), f64: the RMS with the sum.
    pub sum_sq: bool,
    /// Samples clipped (at or past the [ClipRange], any component) or NaN.
    pub clipped: bool,
    /// The OR of the flags words the writer was handed with the node's samples.
    pub flags: bool,
    /// The mean of `I² + Q²` (complex elements), f32.
    pub power_mean: bool,
    /// The largest `I² + Q²` (complex elements), f32: max hold, so short bursts show at every level.
    pub power_max: bool,
}

/// One statistic of [SeriesStats].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Stat {
    Min,
    Max,
    Sum,
    SumSq,
    Clipped,
    Flags,
    PowerMean,
    PowerMax,
}

impl Stat {
    /// Every statistic, in the order a summary block stores them.
    pub const ALL: [Stat; 8] = [
        Stat::Min,
        Stat::Max,
        Stat::Sum,
        Stat::SumSq,
        Stat::Clipped,
        Stat::Flags,
        Stat::PowerMean,
        Stat::PowerMax,
    ];

    /// Bytes of one value.
    pub fn width(self) -> usize {
        match self {
            Stat::Min | Stat::Max | Stat::Sum | Stat::SumSq | Stat::Clipped => 8,
            Stat::Flags | Stat::PowerMean | Stat::PowerMax => 4,
        }
    }

    /// Values per channel and node: the element's components for minimum, maximum and sums, else 1.
    pub fn components(self, element: Element) -> u8 {
        match self {
            Stat::Min | Stat::Max | Stat::Sum | Stat::SumSq => components(element),
            _ => 1,
        }
    }

    /// The statistic's bit in the fields word: `1 <<` its place in [Self::ALL].
    fn bit(self) -> u16 {
        match self {
            Stat::Min => 1,
            Stat::Max => 1 << 1,
            Stat::Sum => 1 << 2,
            Stat::SumSq => 1 << 3,
            Stat::Clipped => 1 << 4,
            Stat::Flags => 1 << 5,
            Stat::PowerMean => 1 << 6,
            Stat::PowerMax => 1 << 7,
        }
    }

    /// Bytes per channel and node: [Self::width] times [Self::components].
    fn node_len(self, element: Element) -> usize {
        match self {
            Stat::Min | Stat::Max | Stat::Sum | Stat::SumSq if is_complex(element) => 16,
            _ => self.width(),
        }
    }
}

impl SeriesStats {
    /// Every statistic for an element: power only for complex ones.
    pub fn all(element: Element) -> Self {
        SeriesStats {
            min: true,
            max: true,
            sum: true,
            sum_sq: true,
            clipped: true,
            flags: true,
            power_mean: is_complex(element),
            power_max: is_complex(element),
        }
    }

    pub fn has(self, stat: Stat) -> bool {
        match stat {
            Stat::Min => self.min,
            Stat::Max => self.max,
            Stat::Sum => self.sum,
            Stat::SumSq => self.sum_sq,
            Stat::Clipped => self.clipped,
            Stat::Flags => self.flags,
            Stat::PowerMean => self.power_mean,
            Stat::PowerMax => self.power_max,
        }
    }

    /// Whether no statistic is kept.
    pub fn is_empty(self) -> bool {
        self.bits() == 0
    }

    pub(super) fn bits(self) -> u16 {
        Stat::ALL
            .into_iter()
            .filter(|s| self.has(*s))
            .fold(0, |b, s| b | s.bit())
    }

    pub(super) fn from_bits(v: u16) -> Option<Self> {
        if v >> Stat::ALL.len() != 0 {
            return None;
        }
        let on = |s: Stat| v & s.bit() != 0;
        Some(SeriesStats {
            min: on(Stat::Min),
            max: on(Stat::Max),
            sum: on(Stat::Sum),
            sum_sq: on(Stat::SumSq),
            clipped: on(Stat::Clipped),
            flags: on(Stat::Flags),
            power_mean: on(Stat::PowerMean),
            power_max: on(Stat::PowerMax),
        })
    }

    /// Bytes per channel and node: at most 8 statistics of 16 bytes.
    pub(super) fn node_len(self, element: Element) -> usize {
        Stat::ALL
            .into_iter()
            .filter(|s| self.has(*s))
            .map(|s| s.node_len(element))
            .sum()
    }
}

/// Where a sample counts as clipped: at or below `low`, at or above `high`, or NaN, in the samples' own units.
///
/// Raw sample values on purpose: the limits are compared with the stored samples (an ADC's codes), whatever unit the
/// track gives them.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct ClipRange {
    pub low: f64,
    pub high: f64,
}

/// What summaries a series track keeps: its declaration, as the track definition carries it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct SeriesSummarySpec {
    /// A level-0 node holds `2^base_shift` samples, [Self::BASE_SHIFT_MIN] to [Self::BASE_SHIFT_MAX]; each level up 16
    /// times more. A raw format field: log2 of a sample count.
    pub base_shift: u8,
    pub stats: SeriesStats,
    /// Where samples clip; None: at the scalar type's ends for integers, infinities for floats.
    pub clip: Option<ClipRange>,
}

impl SeriesSummarySpec {
    /// Smallest [Self::base_shift]: 16 samples.
    pub const BASE_SHIFT_MIN: u8 = 4;
    /// Largest [Self::base_shift]: 16 Mi samples.
    pub const BASE_SHIFT_MAX: u8 = 24;
}

/// How a minimum or maximum is stored.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ValueKind {
    Signed,
    Unsigned,
    Float,
}

/// A statistic's value: minimum and maximum in the element's kind, sums and powers as f64, counts and flags as u64.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Value {
    I64(i64),
    U64(u64),
    F64(f64),
}

impl Value {
    pub(super) fn from_bits(element: Element, bits: u64) -> Self {
        match kind(element) {
            ValueKind::Signed => Value::I64(bits.cast_signed()),
            ValueKind::Unsigned => Value::U64(bits),
            ValueKind::Float => Value::F64(f64::from_bits(bits)),
        }
    }

    pub(super) fn to_bits(self) -> u64 {
        match self {
            Value::I64(v) => v.cast_unsigned(),
            Value::U64(v) => v,
            Value::F64(v) => v.to_bits(),
        }
    }

    /// The value as f64, rounded to nearest.
    pub fn as_f64(self) -> f64 {
        match self {
            Value::I64(v) => i64_f64(v),
            Value::U64(v) => u64_f64(v),
            Value::F64(v) => v,
        }
    }

    fn pick(self, other: Value, smaller: bool) -> Value {
        let take_other = match (self, other) {
            (Value::I64(a), Value::I64(b)) => (b < a) == smaller && b != a,
            (Value::U64(a), Value::U64(b)) => (b < a) == smaller && b != a,
            (Value::F64(a), Value::F64(b)) => a.is_nan() || (if smaller { b < a } else { b > a }),
            _ => false,
        };
        if take_other { other } else { self }
    }
}

/// Why statistics at two scales can't be brought to one.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScaleError {
    /// No scale both are whole multiples of: their ratio isn't a fraction of integers (a power of π, a half
    /// exponent), or the common scale is out of a [Magnitude]'s range.
    NoCommonScale,
    /// A factor to the common scale past [FACTOR_MAX], or a minimum or maximum past its 64-bit type there.
    Overflow,
}

/// Largest factor from a scale to the common scale: every factor is then exact as an f64.
pub const FACTOR_MAX: u64 = 1 << 53;

/// The largest scale `a` and `b` are both whole multiples of: `10⁻⁶` for `10⁻³` and `10⁻⁶`, `1/1000` for `1/1000` and
/// `1024`. The scale statistics of samples at both scales are kept in, so minimum and maximum stay exact integers.
pub fn common_scale(a: Magnitude, b: Magnitude) -> Result<Magnitude, ScaleError> {
    if a == b {
        return Ok(a);
    }
    // a / b = p / q in lowest terms, so a = (b / q) · p and b = (b / q) · q
    let (_p, q) = a
        .checked_div(b)
        .ok()
        .and_then(|r| r.to_ratio())
        .ok_or(ScaleError::NoCommonScale)?;
    let q = u64::try_from(q).ok().ok_or(ScaleError::NoCommonScale)?;
    Magnitude::checked_ratio(1, q)
        .and_then(|r| b.checked_mul(r))
        .ok()
        .ok_or(ScaleError::NoCommonScale)
}

/// `scale / common` as a whole number, at most [FACTOR_MAX].
pub fn scale_factor(scale: Magnitude, common: Magnitude) -> Result<u64, ScaleError> {
    if scale == common {
        return Ok(1);
    }
    let (n, d) = scale
        .checked_div(common)
        .ok()
        .and_then(|r| r.to_ratio())
        .ok_or(ScaleError::NoCommonScale)?;
    if d != 1 {
        return Err(ScaleError::NoCommonScale);
    }
    u64::try_from(n)
        .ok()
        .filter(|n| *n <= FACTOR_MAX)
        .ok_or(ScaleError::Overflow)
}

impl Value {
    /// The value times `k`, exact for integers.
    fn times(self, k: u64) -> Result<Value, ScaleError> {
        Ok(match self {
            Value::I64(v) => Value::I64(
                i128::from(v)
                    .checked_mul(i128::from(k))
                    .and_then(|x| i64::try_from(x).ok())
                    .ok_or(ScaleError::Overflow)?,
            ),
            Value::U64(v) => Value::U64(v.checked_mul(k).ok_or(ScaleError::Overflow)?),
            Value::F64(v) => Value::F64(v * u64_f64(k)),
        })
    }

    /// Whether `self` (a minimum) and `max` are of no samples: the minimum above the maximum, as
    /// [ChannelStats::empty] has them. NaN for floats rescales to NaN, so floats never count as empty here.
    fn no_samples(self, max: Value) -> bool {
        match (self, max) {
            (Value::I64(a), Value::I64(b)) => a > b,
            (Value::U64(a), Value::U64(b)) => a > b,
            _ => false,
        }
    }
}

/// Larger of two floats, NaN left out; the first on a tie, so the result is the same on every platform.
fn fmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b > a { b } else { a }
}

/// The statistics of one channel over a node, or over consecutive nodes merged.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ChannelStats {
    /// Per component (I, Q); only the first is used for a real element.
    pub min: [Value; 2],
    pub max: [Value; 2],
    pub sum: [f64; 2],
    pub sum_sq: [f64; 2],
    pub clipped: u64,
    pub flags: u32,
    pub power_mean: f32,
    pub power_max: f32,
}

impl ChannelStats {
    /// Statistics of no samples: what merging starts from, and what a block that doesn't hold a statistic reads as.
    pub fn empty(element: Element) -> Self {
        let (min, max) = match kind(element) {
            ValueKind::Signed => (Value::I64(i64::MAX), Value::I64(i64::MIN)),
            ValueKind::Unsigned => (Value::U64(u64::MAX), Value::U64(0)),
            ValueKind::Float => (Value::F64(f64::NAN), Value::F64(f64::NAN)),
        };
        ChannelStats {
            min: [min; 2],
            max: [max; 2],
            sum: [0.0; 2],
            sum_sq: [0.0; 2],
            clipped: 0,
            flags: 0,
            power_mean: 0.0,
            power_max: f32::NAN,
        }
    }

    /// Merge `next` (`next_samples` samples, right after this one's `samples`) into this one. Merging in item order,
    /// one node at a time, gives the same bits as the writer's parent node.
    pub fn merge(&mut self, next: &ChannelStats, samples: u64, next_samples: u64) {
        for (a, b) in self.min.iter_mut().zip(next.min) {
            *a = a.pick(b, true);
        }
        for (a, b) in self.max.iter_mut().zip(next.max) {
            *a = a.pick(b, false);
        }
        for (a, b) in self.sum.iter_mut().zip(next.sum) {
            *a += b;
        }
        for (a, b) in self.sum_sq.iter_mut().zip(next.sum_sq) {
            *a += b;
        }
        // a count of samples: a damaged block's count stops at u64::MAX instead of wrapping
        self.clipped = self.clipped.saturating_add(next.clipped);
        self.flags |= next.flags;
        // the counts as f64: exact up to 2^53 samples, so the mean is the same as with an integer total
        let (n, next_n) = (u64_f64(samples), u64_f64(next_samples));
        let total = n + next_n;
        if total > 0.0 {
            let a = f64::from(self.power_mean) * n;
            let b = f64::from(next.power_mean) * next_n;
            self.power_mean = f64_f32((a + b) / total);
        }
        self.power_max = f64_f32(fmax(f64::from(self.power_max), f64::from(next.power_max)));
    }

    /// The statistics with every stored number times `k`: from a scale to a scale `k` times finer. Minimum and
    /// maximum stay exact integers (an error past their 64-bit type), sums are multiplied by `k`, sums of squares
    /// and powers by `k²`; clipped counts and flags don't depend on the scale.
    pub fn rescaled(&self, k: u64) -> Result<Self, ScaleError> {
        if k == 1 {
            return Ok(*self);
        }
        if k > FACTOR_MAX {
            return Err(ScaleError::Overflow);
        }
        let mut s = *self;
        // k <= 2^53: exact as an f64
        let kf = u64_f64(k);
        for (min, max) in s.min.iter_mut().zip(&mut s.max) {
            if !min.no_samples(*max) {
                *min = min.times(k)?;
                *max = max.times(k)?;
            }
        }
        for v in &mut s.sum {
            *v *= kf;
        }
        for v in &mut s.sum_sq {
            *v *= kf * kf;
        }
        s.power_mean = f64_f32(f64::from(self.power_mean) * (kf * kf));
        s.power_max = f64_f32(f64::from(self.power_max) * (kf * kf));
        Ok(s)
    }

    /// Merge `next` (at `next_scale`, `next_samples` samples, right after this one's `samples` at `scale`) into this
    /// one at the common scale of the two ([common_scale]), which it returns. On an error this one is left as it was.
    pub fn merge_scaled(
        &mut self,
        scale: Magnitude,
        next: &ChannelStats,
        next_scale: Magnitude,
        samples: u64,
        next_samples: u64,
    ) -> Result<Magnitude, ScaleError> {
        let common = common_scale(scale, next_scale)?;
        let mut a = self.rescaled(scale_factor(scale, common)?)?;
        let b = next.rescaled(scale_factor(next_scale, common)?)?;
        a.merge(&b, samples, next_samples);
        *self = a;
        Ok(common)
    }
}

/// A node of a series: per channel its statistics, all at one scale.
#[cfg(feature = "alloc")]
#[derive(Clone, Debug, PartialEq)]
pub struct SeriesNode {
    /// What a stored number (minimum, maximum, sum) is worth in the sample type's unit; squares and powers are at
    /// its square.
    pub scale: Magnitude,
    /// One per channel.
    pub channels: Vec<ChannelStats>,
}

#[cfg(feature = "alloc")]
impl SeriesNode {
    /// The node at `scale`, which must be a whole fraction of its own ([scale_factor]). On an error it is left as it
    /// was.
    pub fn rescale(&mut self, scale: Magnitude) -> Result<(), ScaleError> {
        let k = scale_factor(self.scale, scale)?;
        if k != 1 {
            self.channels = self
                .channels
                .iter()
                .map(|c| c.rescaled(k))
                .collect::<Result<_, _>>()?;
        }
        self.scale = scale;
        Ok(())
    }

    /// Merge `next` (`next_samples` samples, right after this one's `samples`, as many channels) into this one, both
    /// at their [common_scale] first. On an error this one is left as it was.
    pub fn merge(
        &mut self,
        next: &SeriesNode,
        samples: u64,
        next_samples: u64,
    ) -> Result<(), ScaleError> {
        let common = common_scale(self.scale, next.scale)?;
        let mut b = next.clone();
        b.rescale(common)?;
        let mut a = self.clone();
        a.rescale(common)?;
        for (x, y) in a.channels.iter_mut().zip(&b.channels) {
            x.merge(y, samples, next_samples);
        }
        *self = a;
        Ok(())
    }
}

#[cfg(feature = "std")]
#[expect(
    private_interfaces,
    reason = "the sealed trait's add_channel takes crate types: no other crate can name or call it"
)]
mod scalar {
    use super::{
        ChannelStats, Element, SeriesStats, Value, components, f64_f32, fmax, i128_f64, is_complex,
        u64_f64,
    };
    use crate::summary::kernel::{IntBatch, Kernel, Narrow, int_channel};

    /// A sample scalar the series writer takes: the Rust type of the scalar of an [Element].
    pub trait Scalar: Copy + PartialOrd + 'static + private::Sealed {
        /// The [Element] of a real sample of this type.
        const TYPE: Element;
        /// The lowest and highest value: the default clip limits.
        const LOWEST: Self;
        const HIGHEST: Self;
        /// Whether squares sum exactly in i128 (integers up to 32 bits).
        const SQ_EXACT: bool;
        /// Whether the type is an integer: sums are exact in i128.
        const INT: bool;
        fn to_f64(self) -> f64;
        /// The value as i128; 0 for floats.
        fn to_i128(self) -> i128;
        fn value(self) -> Value;
        /// `v` rounded down (`up` false) or up, saturating; floats as they are.
        fn from_f64(v: f64, up: bool) -> Self;
    }

    mod private {
        use super::{Acc, Channel, Kernel};

        pub trait Sealed: Sized {
            /// Add one channel's samples of `xs` to `acc` in one pass.
            fn add_channel(acc: &mut Acc<Self>, xs: &[Self], ch: Channel<Self>, kernel: Kernel);
        }
    }

    macro_rules! int_scalar {
        ($t:ty, $ty:ident, $kind:ident, $wide:ty, $exact:expr, $to_f64:path, $add:ident, $($lint:path),+) => {
            impl private::Sealed for $t {
                #[inline]
                fn add_channel(acc: &mut Acc<Self>, xs: &[Self], ch: Channel<Self>, kernel: Kernel) {
                    $add(acc, xs, ch, kernel);
                }
            }
            impl Scalar for $t {
                const TYPE: Element = Element::$ty;
                const LOWEST: Self = <$t>::MIN;
                const HIGHEST: Self = <$t>::MAX;
                const SQ_EXACT: bool = $exact;
                const INT: bool = true;
                #[inline(always)]
                fn to_f64(self) -> f64 {
                    $to_f64(self)
                }
                #[inline(always)]
                fn to_i128(self) -> i128 {
                    i128::from(self)
                }
                fn value(self) -> Value {
                    Value::$kind(<$wide>::from(self))
                }
                #[expect($($lint),+, reason = "saturating, NaN to 0, is the meaning: what `as` does")]
                fn from_f64(v: f64, up: bool) -> Self {
                    if up { v.ceil() as $t } else { v.floor() as $t }
                }
            }
        };
    }

    int_scalar!(
        i8,
        I8,
        I64,
        i64,
        true,
        f64::from,
        narrow_add,
        clippy::cast_possible_truncation
    );
    int_scalar!(
        i16,
        I16,
        I64,
        i64,
        true,
        f64::from,
        narrow_add,
        clippy::cast_possible_truncation
    );
    int_scalar!(
        i32,
        I32,
        I64,
        i64,
        true,
        f64::from,
        narrow_add,
        clippy::cast_possible_truncation
    );
    int_scalar!(
        i64,
        I64,
        I64,
        i64,
        false,
        super::i64_f64,
        ordered_add,
        clippy::cast_possible_truncation
    );
    int_scalar!(
        u8,
        U8,
        U64,
        u64,
        true,
        f64::from,
        narrow_add,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    );
    int_scalar!(
        u16,
        U16,
        U64,
        u64,
        true,
        f64::from,
        narrow_add,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    );
    int_scalar!(
        u32,
        U32,
        U64,
        u64,
        true,
        f64::from,
        narrow_add,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    );
    int_scalar!(
        u64,
        U64,
        U64,
        u64,
        false,
        super::u64_f64,
        ordered_add,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    );

    macro_rules! float_scalar {
        ($t:ty, $ty:ident, $from_f64:path) => {
            impl private::Sealed for $t {
                #[inline]
                fn add_channel(
                    acc: &mut Acc<Self>,
                    xs: &[Self],
                    ch: Channel<Self>,
                    kernel: Kernel,
                ) {
                    ordered_add(acc, xs, ch, kernel);
                }
            }
            impl Scalar for $t {
                const TYPE: Element = Element::$ty;
                const LOWEST: Self = <$t>::NEG_INFINITY;
                const HIGHEST: Self = <$t>::INFINITY;
                const SQ_EXACT: bool = false;
                const INT: bool = false;
                #[inline(always)]
                fn to_f64(self) -> f64 {
                    f64::from(self)
                }
                #[inline(always)]
                fn to_i128(self) -> i128 {
                    0
                }
                fn value(self) -> Value {
                    Value::F64(f64::from(self))
                }
                fn from_f64(v: f64, _up: bool) -> Self {
                    $from_f64(v)
                }
            }
        };
    }

    float_scalar!(f32, F32, super::f64_f32);
    float_scalar!(f64, F64, core::convert::identity);

    /// Running statistics of one component of one channel over the open level-0 node.
    ///
    /// Integer sums are exact: a node holds at most `2^BASE_SHIFT_MAX` (2^24) samples, so a sum of 64-bit values stays
    /// under 2^88 and a sum of squares of values up to 32 bits under 2^88, far inside an i128; the wrapping ops never
    /// wrap.
    #[derive(Copy, Clone, Debug)]
    struct Part<T> {
        min: Option<T>,
        max: Option<T>,
        isum: i128,
        fsum: f64,
        isq: i128,
        fsq: f64,
    }

    impl<T: Scalar> Part<T> {
        const NEW: Self = Part {
            min: None,
            max: None,
            isum: 0,
            fsum: 0.0,
            isq: 0,
            fsq: 0.0,
        };
    }

    /// Where one channel's samples sit in a batch of interleaved frames, and what to keep of them.
    #[derive(Copy, Clone, Debug)]
    pub(crate) struct Channel<T> {
        /// Scalars per frame.
        pub(crate) stride: usize,
        /// The channel's first scalar in a frame.
        pub(crate) at: usize,
        /// Whether a sample is an I/Q pair.
        pub(crate) complex: bool,
        /// Clip limits: a component at or past one counts.
        pub(crate) lo: T,
        pub(crate) hi: T,
        pub(crate) stats: SeriesStats,
    }

    /// Add one channel's samples of `xs` to `acc` in one pass, through `kernel` for integers up to 32 bits.
    #[cfg(test)]
    pub(crate) fn add_channel<T: Scalar>(
        acc: &mut Acc<T>,
        xs: &[T],
        ch: Channel<T>,
        kernel: Kernel,
    ) {
        T::add_channel(acc, xs, ch, kernel);
    }

    /// The samples of `ch` in `xs`, `C` components each: frames too short for them are skipped.
    fn samples<T: Copy, const C: usize>(xs: &[T], ch: Channel<T>) -> impl Iterator<Item = &[T; C]> {
        xs.chunks_exact(ch.stride)
            .filter_map(move |f| f.get(ch.at..).and_then(|f| f.first_chunk::<C>()))
    }

    /// Add the samples of `ch` (integers up to 32 bits) through `kernel`: every statistic is exact, so the batch's
    /// totals are the same in any order of addition.
    fn narrow_add<T: Scalar + Narrow>(acc: &mut Acc<T>, xs: &[T], ch: Channel<T>, kernel: Kernel) {
        debug_assert!(T::INT && T::SQ_EXACT, "a narrow type's squares sum exactly");
        let mut b = IntBatch::NEW;
        if ch.complex {
            int_channel::<T, 2>(kernel, xs, ch.stride, ch.at, ch.lo, ch.hi, &mut b);
        } else {
            int_channel::<T, 1>(kernel, xs, ch.stride, ch.at, ch.lo, ch.hi, &mut b);
        }
        if b.frames == 0 {
            return;
        }
        let stats = ch.stats;
        let comps = if ch.complex { 2 } else { 1 };
        let batch = b.min.iter().zip(&b.max).zip(b.sum.iter().zip(&b.sq));
        for (part, ((&min, &max), (&sum, &sq))) in acc.parts.iter_mut().take(comps).zip(batch) {
            if stats.min || stats.max {
                part.min = Some(part.min.map_or(min, |m| m.min(min)));
                part.max = Some(part.max.map_or(max, |m| m.max(max)));
            }
            if stats.sum || stats.sum_sq {
                // under 2^88 and 2^89 over a node of 2^24 samples (see Part)
                part.isum = part.isum.wrapping_add(sum);
                part.isq = part.isq.wrapping_add(sq);
            }
        }
        if ch.complex && (stats.power_mean || stats.power_max) {
            acc.ipow = acc.ipow.wrapping_add(b.pow);
            // rounding is monotonic: the largest power rounded is the largest of the powers rounded
            acc.pmax = fmax(acc.pmax, i128_f64(b.pmax));
        }
        if stats.clipped {
            acc.clipped = acc.clipped.saturating_add(b.clipped);
        }
    }

    /// Add the samples of `ch` (64-bit integers and floats) one frame at a time, in their order: float sums depend on
    /// it. Per component, float sums and sums of squares add each sample to the node's running sum in sample order;
    /// the squares of a 64-bit integer add to a sum of the batch first, which then adds to the node's.
    fn ordered_add<T: Scalar>(acc: &mut Acc<T>, xs: &[T], ch: Channel<T>, _kernel: Kernel) {
        if ch.complex {
            ordered::<T, 2>(acc, xs, ch);
        } else {
            ordered::<T, 1>(acc, xs, ch);
        }
    }

    #[inline(always)]
    fn ordered<T: Scalar, const C: usize>(acc: &mut Acc<T>, xs: &[T], ch: Channel<T>) {
        let stats = ch.stats;
        let (minmax, sums) = (stats.min || stats.max, stats.sum || stats.sum_sq);
        let power = C == 2 && (stats.power_mean || stats.power_max);
        // squares of a 64-bit integer, summed over the batch
        let mut fq = [0.0f64; C];
        for x in samples::<T, C>(xs, ch) {
            for ((&x, part), fq) in x.iter().zip(&mut acc.parts).zip(&mut fq) {
                // NaN compares with nothing, itself included
                if minmax && x.partial_cmp(&x).is_some() {
                    if part.min.is_none_or(|m| x < m) {
                        part.min = Some(x);
                    }
                    if part.max.is_none_or(|m| x > m) {
                        part.max = Some(x);
                    }
                }
                if sums {
                    if T::INT {
                        // under 2^88 over a node (see Part)
                        part.isum = part.isum.wrapping_add(x.to_i128());
                        if T::SQ_EXACT {
                            let v = x.to_i128();
                            part.isq = part.isq.wrapping_add(v.wrapping_mul(v));
                        } else {
                            let f = x.to_f64();
                            *fq += f * f;
                        }
                    } else {
                        let f = x.to_f64();
                        part.fsum += f;
                        part.fsq += f * f;
                    }
                }
            }
            if power && let Some(&[i, q]) = x.first_chunk::<2>() {
                let p = if T::SQ_EXACT {
                    // values up to 32 bits: a power under 2^65, a node's sum under 2^89
                    let (i, q) = (i.to_i128(), q.to_i128());
                    let p = i.wrapping_mul(i).wrapping_add(q.wrapping_mul(q));
                    acc.ipow = acc.ipow.wrapping_add(p);
                    i128_f64(p)
                } else {
                    let (i, q) = (i.to_f64(), q.to_f64());
                    let p = i * i + q * q;
                    acc.fpow += p;
                    p
                };
                acc.pmax = fmax(acc.pmax, p);
            }
            if stats.clipped && x.iter().any(|&v| !(ch.lo < v && v < ch.hi)) {
                // at most a node's 2^24 samples
                acc.clipped = acc.clipped.saturating_add(1);
            }
        }
        if sums && T::INT {
            for (part, fq) in acc.parts.iter_mut().zip(fq) {
                part.fsq += fq;
            }
        }
    }

    /// Running statistics of one channel over the open level-0 node.
    #[derive(Copy, Clone, Debug)]
    pub(crate) struct Acc<T> {
        parts: [Part<T>; 2],
        /// Sum of the powers of an integer element: exact, as the sums of [Part]
        ipow: i128,
        fpow: f64,
        pmax: f64,
        clipped: u64,
        pub(super) flags: u32,
    }

    impl<T: Scalar> Acc<T> {
        pub(crate) const NEW: Self = Acc {
            parts: [Part::NEW; 2],
            ipow: 0,
            fpow: 0.0,
            pmax: f64::NAN,
            clipped: 0,
            flags: 0,
        };

        pub(crate) fn stats(&self, element: Element, samples: u64) -> ChannelStats {
            let mut s = ChannelStats::empty(element);
            let slots = s
                .min
                .iter_mut()
                .zip(&mut s.max)
                .zip(s.sum.iter_mut().zip(&mut s.sum_sq));
            let parts = self.parts.iter().take(usize::from(components(element)));
            for (part, ((min, max), (sum, sum_sq))) in parts.zip(slots) {
                if let Some(v) = part.min {
                    *min = v.value();
                }
                if let Some(v) = part.max {
                    *max = v.value();
                }
                if T::INT {
                    *sum = i128_f64(part.isum);
                    *sum_sq = if T::SQ_EXACT {
                        i128_f64(part.isq)
                    } else {
                        part.fsq
                    };
                } else {
                    *sum = part.fsum;
                    *sum_sq = part.fsq;
                }
            }
            s.clipped = self.clipped;
            s.flags = self.flags;
            if is_complex(element) && samples > 0 {
                let p = if T::SQ_EXACT {
                    i128_f64(self.ipow)
                } else {
                    self.fpow
                };
                s.power_mean = f64_f32(p / u64_f64(samples));
                s.power_max = f64_f32(self.pmax);
            }
            s
        }
    }
}
#[cfg(feature = "std")]
pub use scalar::Scalar;
#[cfg(all(test, feature = "std"))]
pub(crate) use scalar::{Acc, Channel, add_channel};

#[cfg(feature = "std")]
pub use builder::{SERIES_BLOCK_BYTES, SeriesStart, SeriesSummaries};

#[cfg(feature = "std")]
mod builder {
    use exact_units::Magnitude;

    use super::scalar::{Acc, Channel};
    use super::{
        ChannelStats, Scalar, SeriesNode, SeriesSummarySpec, common_scale, components, is_complex,
    };
    use crate::summary::kernel::Kernel;
    use crate::summary::levels::{LevelNode, Levels, Output};
    use crate::summary::{
        Layout, SCALE_LEN_MAX, SummaryError, SummaryHead, Tree, summary_len, write_series_summary,
    };
    use crate::{Element, Ratio64, SampleShape, SeriesSegment, SeriesTimeline};

    impl LevelNode for SeriesNode {
        fn merge(&mut self, next: &Self, items: u64, next_items: u64) -> Result<(), SummaryError> {
            SeriesNode::merge(self, next, items, next_items).map_err(SummaryError::Scale)
        }
    }

    /// Builds the summary tree of one series track as samples come, and encodes its summary blocks.
    ///
    /// Samples go in as interleaved frames ([Self::push]): per sample every channel, per channel every component.
    /// A segment ([Self::segment]) may change the scale of the numbers from the next sample on; a node holding
    /// samples of two scales is kept at their [common_scale](super::common_scale), so every node's minimum and maximum
    /// are exact in the sample type's unit. Finished blocks wait in [Self::take_blocks] for
    /// [TapeWriter::defer_block](crate::TapeWriter::defer_block); [Self::finish] writes the partial nodes at the end.
    /// After an error the summaries are no longer whole: drop them.
    ///
    /// Every statistic is kept in one pass over a batch. Integers up to 32 bits sum exactly, so their results don't
    /// depend on how samples are split into pushes; they run through vectorised kernels chosen for the CPU at
    /// [Self::new]. Float sums and sums of squares add each sample in sample order, the squares of 64-bit integers
    /// sum per push first: the same bits for the same pushes, NaN payloads aside.
    pub struct SeriesSummaries<T: Scalar> {
        track_id: u64,
        track_number: u32,
        element: Element,
        channels: u16,
        /// Scalars per sample of one channel: 1, or 2 for a complex element
        components: usize,
        /// Scalars per frame: channels times components
        stride: usize,
        /// Bytes of a node in a block
        node_len: usize,
        /// `2^base_shift - 1`: an item's place in its level-0 node
        node_mask: u64,
        spec: SeriesSummarySpec,
        origin: u64,
        timeline: SeriesTimeline,
        /// Scale of the samples from [Self::acc_from] on
        scale: Magnitude,
        /// Common scale of every sample so far
        common: Magnitude,
        lo: T,
        hi: T,
        /// The open level-0 node's samples since [Self::acc_from]
        acc: Vec<Acc<T>>,
        acc_from: u64,
        /// The open level-0 node's samples before [Self::acc_from], at an earlier scale, and their count
        part: Option<(SeriesNode, u64)>,
        next_item: u64,
        levels: Levels<SeriesNode>,
        blocks: Vec<Vec<u8>>,
        /// The code the integer statistics run through
        kernel: Kernel,
    }

    /// Where a series starts: its track, its samples and the segment of its first sample.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct SeriesStart {
        /// [TrackDef::id](crate::TrackDef::id).
        pub track_id: u64,
        /// [TrackDef::number](crate::TrackDef::number).
        pub track_number: u32,
        /// The samples' element and channels, at most 65535.
        pub shape: SampleShape,
        /// Index of the first sample in the track (a later tape piece starts past 0).
        pub first_item: u64,
        /// The segment the first sample is in: its grid and scale.
        pub segment: SeriesSegment,
        /// Ticks from one sample to the next, exact.
        pub period: Ratio64,
    }

    /// Default most bytes of a series summary block: small enough to fit the gap before an aligned chunk payload.
    pub const SERIES_BLOCK_BYTES: usize = 2048;

    impl<T: Scalar> SeriesSummaries<T> {
        /// Summaries of the series `start` describes, kept as `spec` declares.
        pub fn new(start: SeriesStart, spec: SeriesSummarySpec) -> Result<Self, SummaryError> {
            let element = start.shape.element;
            if element.scalar() != T::TYPE {
                return Err(SummaryError::Invalid(
                    "element is not the samples' scalar type",
                ));
            }
            let channels = u16::try_from(start.shape.channels.0)
                .ok()
                .ok_or(SummaryError::Invalid("more than 65535 channels"))?;
            if channels == 0 {
                return Err(SummaryError::Invalid("a series has no channels"));
            }
            if !(SeriesSummarySpec::BASE_SHIFT_MIN..=SeriesSummarySpec::BASE_SHIFT_MAX)
                .contains(&spec.base_shift)
            {
                return Err(SummaryError::Invalid("base shift out of range"));
            }
            if spec.stats.is_empty() {
                return Err(SummaryError::Invalid("no statistics"));
            }
            if (spec.stats.power_mean || spec.stats.power_max) && !is_complex(element) {
                return Err(SummaryError::Invalid("power of a real element"));
            }
            if start.segment.first_index > start.first_item {
                return Err(SummaryError::Invalid(
                    "the first sample is before its segment",
                ));
            }
            let mut timeline = SeriesTimeline::new(start.period);
            timeline.push(start.segment).map_err(SummaryError::Series)?;
            let (lo, hi) = match spec.clip {
                None => (T::LOWEST, T::HIGHEST),
                Some(c) => (T::from_f64(c.low, false), T::from_f64(c.high, true)),
            };
            let too_wide = SummaryError::Invalid("a frame or node past usize");
            let components = usize::from(components(element));
            let stride = usize::from(channels)
                .checked_mul(components)
                .ok_or(too_wide)?;
            let node_len = spec
                .stats
                .node_len(element)
                .checked_mul(usize::from(channels))
                .ok_or(too_wide)?;
            Ok(SeriesSummaries {
                track_id: start.track_id,
                track_number: start.track_number,
                element,
                channels,
                components,
                stride,
                node_len,
                // base_shift is at most BASE_SHIFT_MAX, checked above
                node_mask: !(u64::MAX << spec.base_shift),
                spec,
                origin: start.first_item,
                timeline,
                scale: start.segment.scale,
                common: start.segment.scale,
                lo,
                hi,
                acc: vec![Acc::NEW; usize::from(channels)],
                acc_from: start.first_item,
                part: None,
                next_item: start.first_item,
                levels: Levels::new(
                    start.first_item,
                    spec.base_shift,
                    node_len,
                    SERIES_BLOCK_BYTES - SCALE_LEN_MAX,
                ),
                blocks: Vec::new(),
                kernel: Kernel::best(),
            })
        }

        /// Keep blocks within `bytes` (default [SERIES_BLOCK_BYTES]); call before the first sample.
        pub fn set_max_block_bytes(&mut self, bytes: usize) {
            self.levels = Levels::new(
                self.origin,
                self.spec.base_shift,
                self.node_len,
                bytes.saturating_sub(SCALE_LEN_MAX),
            );
        }

        /// The next sample's index.
        pub fn next_item(&self) -> u64 {
            self.next_item
        }

        /// The segments seen so far.
        pub fn timeline(&self) -> &SeriesTimeline {
            &self.timeline
        }

        /// Start segment `s` at the next sample ([Self::next_item]): a new grid (a gap, a restart) or a new scale (a
        /// prefix change). Refused when it doesn't start at the next sample, breaks a rule of [SeriesTimeline], or
        /// its scale has no common scale with the ones before.
        pub fn segment(&mut self, s: SeriesSegment) -> Result<(), SummaryError> {
            if s.first_index != self.next_item {
                return Err(SummaryError::Invalid(
                    "a segment that doesn't start at the next sample",
                ));
            }
            let common = common_scale(self.common, s.scale).map_err(SummaryError::Scale)?;
            self.timeline.push(s).map_err(SummaryError::Series)?;
            if s.scale != self.scale {
                self.flush_acc()?;
                self.scale = s.scale;
                self.common = common;
            }
            Ok(())
        }

        /// Add samples: whole frames of `channels × components` scalars, interleaved; `flags` ORs into the nodes they
        /// fall in. Samples that are not whole frames are refused.
        pub fn push(&mut self, samples: &[T], flags: u32) -> Result<(), SummaryError> {
            let stride = self.stride;
            let partial = SummaryError::PartialFrame { stride };
            if stride == 0 || !samples.len().is_multiple_of(stride) {
                return Err(partial);
            }
            let past = SummaryError::Invalid("sample index past u64");
            let mut rest = samples;
            while !rest.is_empty() {
                // the samples left in the open node: 1 to 2^BASE_SHIFT_MAX
                let to_boundary = (!self.next_item & self.node_mask)
                    .checked_add(1)
                    .ok_or(past)?;
                // a boundary past usize is farther than any slice
                let to_boundary = usize::try_from(to_boundary).unwrap_or(usize::MAX);
                let frames = rest
                    .len()
                    .checked_div(stride)
                    .ok_or(partial)?
                    .min(to_boundary);
                let (now, later) = rest
                    .split_at_checked(frames.checked_mul(stride).ok_or(partial)?)
                    .ok_or(partial)?;
                self.add(now, flags);
                self.next_item = u64::try_from(frames)
                    .ok()
                    .and_then(|n| self.next_item.checked_add(n))
                    .ok_or(past)?;
                rest = later;
                if self.next_item & self.node_mask == 0 {
                    self.close_node()?;
                    self.encode()?;
                }
            }
            Ok(())
        }

        fn add(&mut self, xs: &[T], flags: u32) {
            let comps = self.components;
            let stride = self.stride;
            // channel `ch` starts at scalar ch · comps of every frame: the channels' starts step by comps
            let starts = (0..stride).step_by(comps);
            for (acc, at) in self.acc.iter_mut().zip(starts) {
                let ch = Channel {
                    stride,
                    at,
                    complex: is_complex(self.element),
                    lo: self.lo,
                    hi: self.hi,
                    stats: self.spec.stats,
                };
                T::add_channel(acc, xs, ch, self.kernel);
                acc.flags |= flags;
            }
        }

        /// Run the integer kernels as `kernel` from now on.
        #[cfg(test)]
        pub(crate) fn set_kernel(&mut self, kernel: Kernel) {
            self.kernel = kernel;
        }

        /// The open level-0 node's samples since the last change of scale, at their scale, and their count.
        fn acc_node(&self) -> Result<(SeriesNode, u64), SummaryError> {
            let samples =
                self.next_item
                    .checked_sub(self.acc_from)
                    .ok_or(SummaryError::Invalid(
                        "open node starts past the next sample",
                    ))?;
            let channels = self
                .acc
                .iter()
                .map(|a| a.stats(self.element, samples))
                .collect();
            Ok((
                SeriesNode {
                    scale: self.scale,
                    channels,
                },
                samples,
            ))
        }

        /// The open level-0 node so far: the part before the last change of scale merged with the samples since.
        fn open_node(&self) -> Result<Option<SeriesNode>, SummaryError> {
            let (node, samples) = self.acc_node()?;
            Ok(match (&self.part, samples) {
                (None, 0) => None,
                (None, _) => Some(node),
                (Some((p, _)), 0) => Some(p.clone()),
                (Some((p, n)), _) => {
                    let mut p = p.clone();
                    p.merge(&node, *n, samples).map_err(SummaryError::Scale)?;
                    Some(p)
                }
            })
        }

        /// Move the samples since the last change of scale into the open node's earlier part.
        fn flush_acc(&mut self) -> Result<(), SummaryError> {
            let (node, samples) = self.acc_node()?;
            if samples > 0 {
                self.part = Some(match self.part.take() {
                    None => (node, samples),
                    Some((mut p, n)) => {
                        p.merge(&node, n, samples).map_err(SummaryError::Scale)?;
                        let n = n
                            .checked_add(samples)
                            .ok_or(SummaryError::Invalid("open node past u64 samples"))?;
                        (p, n)
                    }
                });
                self.acc.fill(Acc::NEW);
            }
            self.acc_from = self.next_item;
            Ok(())
        }

        /// Close the open level-0 node (full, or partial at the end) and hand it up.
        fn close_node(&mut self) -> Result<(), SummaryError> {
            self.flush_acc()?;
            let (node, _) = self
                .part
                .take()
                .ok_or(SummaryError::Invalid("no open node to close"))?;
            let idx = self
                .last_node()
                .ok_or(SummaryError::Invalid("no sample to close"))?;
            self.levels.push(0, idx, node, self.next_item)
        }

        /// Whether the open level-0 node holds samples.
        fn node_open(&self) -> bool {
            self.next_item > self.origin && self.next_item & self.node_mask != 0
        }

        /// The level-0 node of the last sample; None before the first.
        fn last_node(&self) -> Option<u64> {
            Some(self.next_item.checked_sub(1)? >> self.spec.base_shift)
        }

        /// Write the open nodes of every level, partial ones included, as blocks. Call it once, after the last
        /// sample.
        pub fn finish(&mut self) -> Result<(), SummaryError> {
            if self.node_open() {
                self.close_node()?;
            }
            self.levels.finish(self.next_item)?;
            self.encode()
        }

        fn tick(&self, item: u64) -> Result<i64, SummaryError> {
            self.timeline
                .tick(item)
                .ok_or(SummaryError::Invalid("sample tick past i64"))
        }

        fn encode(&mut self) -> Result<(), SummaryError> {
            for out in self.levels.take_output() {
                let Output {
                    level,
                    first_item,
                    items,
                    mut nodes,
                } = out;
                // one scale per block: the common scale of its nodes
                let mut scale = nodes
                    .first()
                    .map(|n| n.scale)
                    .ok_or(SummaryError::Invalid("a batch holds nodes"))?;
                for n in &nodes {
                    scale = common_scale(scale, n.scale).map_err(SummaryError::Scale)?;
                }
                for n in &mut nodes {
                    n.rescale(scale).map_err(SummaryError::Scale)?;
                }
                let last = items
                    .checked_sub(1)
                    .and_then(|n| first_item.checked_add(n))
                    .ok_or(SummaryError::Invalid("a batch of no items"))?;
                let last_tick = self.tick(last)?;
                let head = SummaryHead {
                    track_id: self.track_id,
                    track_number: self.track_number,
                    tree: Tree::Series,
                    level,
                    node_shift: self.levels.shift(level)?,
                    layout: Layout::Series {
                        element: self.element,
                        channels: self.channels,
                        stats: self.spec.stats,
                        scale,
                    },
                    first_item,
                    items,
                    first_tick: self.tick(first_item)?,
                    last_tick,
                    latest_end: last_tick,
                };
                let refs: Vec<&[ChannelStats]> =
                    nodes.iter().map(|n| n.channels.as_slice()).collect();
                let mut buf = vec![0u8; summary_len(&head)?];
                let len = write_series_summary(
                    &mut buf,
                    crate::BlockId::new(crate::SessionTag::NONE, 0),
                    &head,
                    &refs,
                )?;
                buf.truncate(len);
                self.blocks.push(buf);
            }
            Ok(())
        }

        /// Finished summary blocks, oldest first, for
        /// [TapeWriter::defer_block](crate::TapeWriter::defer_block).
        pub fn take_blocks(&mut self) -> Vec<Vec<u8>> {
            core::mem::take(&mut self.blocks)
        }

        /// What a live reader takes from memory for `level`: the number of the first node not in a block yet, and the
        /// nodes from it on, each at its own scale; the last may be partial (the open node).
        pub fn pending(&self, level: u8) -> Result<(u64, Vec<SeriesNode>), SummaryError> {
            let (mut first, mut nodes) = self.levels.pending(level);
            if level == 0
                && self.node_open()
                && let Some(open) = self.open_node()?
            {
                if nodes.is_empty() {
                    first = self
                        .last_node()
                        .ok_or(SummaryError::Invalid("an open node of no samples"))?;
                }
                nodes.push(open);
            }
            Ok((first, nodes))
        }
    }
}
