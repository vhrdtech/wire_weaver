//! Series summaries: the statistics a series track declares, per channel and node, and how they merge.

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

    fn bit(self) -> u16 {
        1 << Stat::ALL.iter().position(|s| *s == self).expect("in ALL")
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

    /// Bytes per channel and node.
    pub(super) fn node_len(self, element: Element) -> usize {
        Stat::ALL
            .into_iter()
            .filter(|s| self.has(*s))
            .map(|s| s.width() * usize::from(s.components(element)))
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
            ValueKind::Signed => Value::I64(bits as i64),
            ValueKind::Unsigned => Value::U64(bits),
            ValueKind::Float => Value::F64(f64::from_bits(bits)),
        }
    }

    pub(super) fn to_bits(self) -> u64 {
        match self {
            Value::I64(v) => v as u64,
            Value::U64(v) => v,
            Value::F64(v) => v.to_bits(),
        }
    }

    /// The value as f64, rounded to nearest.
    pub fn as_f64(self) -> f64 {
        match self {
            Value::I64(v) => v as f64,
            Value::U64(v) => v as f64,
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
        for c in 0..2 {
            self.min[c] = self.min[c].pick(next.min[c], true);
            self.max[c] = self.max[c].pick(next.max[c], false);
            self.sum[c] += next.sum[c];
            self.sum_sq[c] += next.sum_sq[c];
        }
        self.clipped += next.clipped;
        self.flags |= next.flags;
        let total = samples + next_samples;
        if total > 0 {
            let a = f64::from(self.power_mean) * samples as f64;
            let b = f64::from(next.power_mean) * next_samples as f64;
            self.power_mean = ((a + b) / total as f64) as f32;
        }
        self.power_max = fmax(f64::from(self.power_max), f64::from(next.power_max)) as f32;
    }
}

#[cfg(feature = "std")]
mod scalar {
    use super::{ChannelStats, Element, SeriesStats, Value, components, fmax, is_complex};

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
        pub trait Sealed {}
    }

    macro_rules! int_scalar {
        ($t:ty, $ty:ident, $kind:ident, $wide:ty, $exact:expr) => {
            impl private::Sealed for $t {}
            impl Scalar for $t {
                const TYPE: Element = Element::$ty;
                const LOWEST: Self = <$t>::MIN;
                const HIGHEST: Self = <$t>::MAX;
                const SQ_EXACT: bool = $exact;
                const INT: bool = true;
                #[inline(always)]
                fn to_f64(self) -> f64 {
                    self as f64
                }
                #[inline(always)]
                fn to_i128(self) -> i128 {
                    i128::from(self)
                }
                fn value(self) -> Value {
                    Value::$kind(<$wide>::from(self))
                }
                fn from_f64(v: f64, up: bool) -> Self {
                    // `as` saturates and maps NaN to 0
                    if up { v.ceil() as $t } else { v.floor() as $t }
                }
            }
        };
    }

    int_scalar!(i8, I8, I64, i64, true);
    int_scalar!(i16, I16, I64, i64, true);
    int_scalar!(i32, I32, I64, i64, true);
    int_scalar!(i64, I64, I64, i64, false);
    int_scalar!(u8, U8, U64, u64, true);
    int_scalar!(u16, U16, U64, u64, true);
    int_scalar!(u32, U32, U64, u64, true);
    int_scalar!(u64, U64, U64, u64, false);

    macro_rules! float_scalar {
        ($t:ty, $ty:ident) => {
            impl private::Sealed for $t {}
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
                    v as $t
                }
            }
        };
    }

    float_scalar!(f32, F32);
    float_scalar!(f64, F64);

    /// Running statistics of one channel over the open level-0 node.
    #[derive(Copy, Clone, Debug)]
    pub(super) struct Acc<T> {
        min: [Option<T>; 2],
        max: [Option<T>; 2],
        isum: [i128; 2],
        fsum: [f64; 2],
        isq: [i128; 2],
        fsq: [f64; 2],
        ipow: i128,
        fpow: f64,
        pmax: f64,
        clipped: u64,
        pub(super) flags: u32,
    }

    impl<T: Scalar> Acc<T> {
        pub(super) const NEW: Self = Acc {
            min: [None; 2],
            max: [None; 2],
            isum: [0; 2],
            fsum: [0.0; 2],
            isq: [0; 2],
            fsq: [0.0; 2],
            ipow: 0,
            fpow: 0.0,
            pmax: f64::NAN,
            clipped: 0,
            flags: 0,
        };

        /// Add the samples of component `c` (every `stride`-th scalar of `xs`).
        #[inline(always)]
        pub(super) fn add_component(
            &mut self,
            xs: &[T],
            stride: usize,
            c: usize,
            stats: SeriesStats,
        ) {
            if stats.min || stats.max {
                let (mut lo, mut hi) = (self.min[c], self.max[c]);
                for &x in xs.iter().skip(c).step_by(stride) {
                    // NaN compares with nothing, itself included
                    if x.partial_cmp(&x).is_none() {
                        continue;
                    }
                    if lo.is_none_or(|m| x < m) {
                        lo = Some(x);
                    }
                    if hi.is_none_or(|m| x > m) {
                        hi = Some(x);
                    }
                }
                (self.min[c], self.max[c]) = (lo, hi);
            }
            if stats.sum || stats.sum_sq {
                if T::INT {
                    let (mut s, mut q) = (0i128, 0i128);
                    let mut fq = 0.0f64;
                    for &x in xs.iter().skip(c).step_by(stride) {
                        let v = x.to_i128();
                        s += v;
                        if T::SQ_EXACT {
                            q += v * v;
                        } else {
                            let f = x.to_f64();
                            fq += f * f;
                        }
                    }
                    self.isum[c] += s;
                    self.isq[c] += q;
                    self.fsq[c] += fq;
                } else {
                    for &x in xs.iter().skip(c).step_by(stride) {
                        let f = x.to_f64();
                        self.fsum[c] += f;
                        self.fsq[c] += f * f;
                    }
                }
            }
        }

        /// Add the power `I² + Q²` of the complex samples at `at` in each frame of `stride` scalars of `xs`.
        pub(super) fn add_power(&mut self, xs: &[T], stride: usize, at: usize) {
            for frame in xs.chunks_exact(stride) {
                let pair = &frame[at..at + 2];
                let (i, q) = (pair[0], pair[1]);
                let p = if T::SQ_EXACT {
                    let (i, q) = (i.to_i128(), q.to_i128());
                    let p = i * i + q * q;
                    self.ipow += p;
                    p as f64
                } else {
                    let (i, q) = (i.to_f64(), q.to_f64());
                    let p = i * i + q * q;
                    self.fpow += p;
                    p
                };
                self.pmax = fmax(self.pmax, p);
            }
        }

        /// Count the samples (`components` scalars at `at` in each frame of `stride` scalars of `xs`) clipped at
        /// `lo`/`hi`.
        pub(super) fn add_clipped(
            &mut self,
            xs: &[T],
            stride: usize,
            at: usize,
            components: usize,
            lo: T,
            hi: T,
        ) {
            for frame in xs.chunks_exact(stride) {
                if frame[at..at + components]
                    .iter()
                    .any(|&x| !(lo < x && x < hi))
                {
                    self.clipped += 1;
                }
            }
        }

        pub(super) fn stats(&self, element: Element, samples: u64) -> ChannelStats {
            let mut s = ChannelStats::empty(element);
            for c in 0..usize::from(components(element)) {
                if let Some(v) = self.min[c] {
                    s.min[c] = v.value();
                }
                if let Some(v) = self.max[c] {
                    s.max[c] = v.value();
                }
                if T::INT {
                    s.sum[c] = self.isum[c] as f64;
                    s.sum_sq[c] = if T::SQ_EXACT {
                        self.isq[c] as f64
                    } else {
                        self.fsq[c]
                    };
                } else {
                    s.sum[c] = self.fsum[c];
                    s.sum_sq[c] = self.fsq[c];
                }
            }
            s.clipped = self.clipped;
            s.flags = self.flags;
            if is_complex(element) && samples > 0 {
                let p = if T::SQ_EXACT {
                    self.ipow as f64
                } else {
                    self.fpow
                };
                s.power_mean = (p / samples as f64) as f32;
                s.power_max = self.pmax as f32;
            }
            s
        }
    }
}
#[cfg(feature = "std")]
pub use scalar::Scalar;

#[cfg(feature = "std")]
pub use builder::{SERIES_BLOCK_BYTES, SeriesStart, SeriesSummaries};

#[cfg(feature = "std")]
mod builder {
    use super::scalar::Acc;
    use super::{ChannelStats, Scalar, SeriesSummarySpec, components, is_complex};
    use crate::summary::levels::{LevelNode, Levels, Output};
    use crate::summary::{Layout, SummaryError, SummaryHead, Tree, write_series_summary};
    use crate::{Element, Ratio64, SampleShape};
    use exact_units::Rounding;

    /// A level's node of a series: one [ChannelStats] per channel.
    #[derive(Clone, Debug)]
    struct Node(Vec<ChannelStats>);

    impl LevelNode for Node {
        fn merge(&mut self, next: &Self, items: u64, next_items: u64) {
            for (a, b) in self.0.iter_mut().zip(&next.0) {
                a.merge(b, items, next_items);
            }
        }
    }

    /// Builds the summary tree of one series track as samples come, and encodes its summary blocks.
    ///
    /// Samples go in as interleaved frames ([Self::push]): per sample every channel, per channel every component.
    /// Finished blocks wait in [Self::take_blocks] for
    /// [TapeWriter::defer_block](crate::TapeWriter::defer_block); [Self::finish] writes the partial nodes at the end.
    pub struct SeriesSummaries<T: Scalar> {
        track_id: u64,
        track_number: u32,
        element: Element,
        channels: u16,
        spec: SeriesSummarySpec,
        origin: u64,
        first_tick: i64,
        period: Ratio64,
        lo: T,
        hi: T,
        acc: Vec<Acc<T>>,
        next_item: u64,
        levels: Levels<Node>,
        blocks: Vec<Vec<u8>>,
    }

    /// Where a series starts: its track, its samples and the tick of its first sample.
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
        /// Tick of the first sample.
        pub first_tick: i64,
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
                .map_err(|_e| SummaryError::Invalid("more than 65535 channels"))?;
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
            let (lo, hi) = match spec.clip {
                None => (T::LOWEST, T::HIGHEST),
                Some(c) => (T::from_f64(c.low, false), T::from_f64(c.high, true)),
            };
            let node_len = spec.stats.node_len(element) * usize::from(channels);
            Ok(SeriesSummaries {
                track_id: start.track_id,
                track_number: start.track_number,
                element,
                channels,
                spec,
                origin: start.first_item,
                first_tick: start.first_tick,
                period: start.period,
                lo,
                hi,
                acc: vec![Acc::NEW; usize::from(channels)],
                next_item: start.first_item,
                levels: Levels::new(
                    start.first_item,
                    spec.base_shift,
                    node_len,
                    SERIES_BLOCK_BYTES,
                ),
                blocks: Vec::new(),
            })
        }

        /// Keep blocks within `bytes` (default [SERIES_BLOCK_BYTES]); call before the first sample.
        pub fn set_max_block_bytes(&mut self, bytes: usize) {
            let node_len = self.spec.stats.node_len(self.element) * usize::from(self.channels);
            self.levels = Levels::new(self.origin, self.spec.base_shift, node_len, bytes);
        }

        /// The next sample's index.
        pub fn next_item(&self) -> u64 {
            self.next_item
        }

        /// Add samples: whole frames of `channels × components` scalars, interleaved; `flags` ORs into the nodes they
        /// fall in.
        ///
        /// # Panics
        ///
        /// When `samples` is not whole frames.
        pub fn push(&mut self, samples: &[T], flags: u32) -> Result<(), SummaryError> {
            let stride = usize::from(self.channels) * usize::from(components(self.element));
            assert!(
                samples.len().is_multiple_of(stride),
                "samples are whole frames of {stride} scalars"
            );
            let mut rest = samples;
            let node = 1u64 << self.spec.base_shift;
            while !rest.is_empty() {
                let to_boundary = node - (self.next_item & (node - 1));
                let frames = (rest.len() / stride).min(to_boundary as usize);
                let (now, later) = rest.split_at(frames * stride);
                self.add(now, flags);
                self.next_item += frames as u64;
                rest = later;
                if self.next_item & (node - 1) == 0 {
                    self.close_node();
                    self.encode()?;
                }
            }
            Ok(())
        }

        fn add(&mut self, xs: &[T], flags: u32) {
            let comps = usize::from(components(self.element));
            let stride = usize::from(self.channels) * comps;
            let stats = self.spec.stats;
            for (ch, acc) in self.acc.iter_mut().enumerate() {
                for c in 0..comps {
                    acc.add_component(&xs[ch * comps..], stride, c, stats);
                }
                if is_complex(self.element) && (stats.power_mean || stats.power_max) {
                    acc.add_power(xs, stride, ch * comps);
                }
                if stats.clipped {
                    acc.add_clipped(xs, stride, ch * comps, comps, self.lo, self.hi);
                }
                acc.flags |= flags;
            }
        }

        /// Close the open level-0 node (full, or partial at the end) and hand it up.
        fn close_node(&mut self) {
            let shift = self.spec.base_shift;
            let idx = (self.next_item - 1) >> shift;
            let from = (idx << shift).max(self.origin);
            let samples = self.next_item - from;
            let node = Node(
                self.acc
                    .iter()
                    .map(|a| a.stats(self.element, samples))
                    .collect(),
            );
            self.acc.fill(Acc::NEW);
            self.levels.push(0, idx, node, self.next_item);
        }

        /// Whether the open level-0 node holds samples.
        fn node_open(&self) -> bool {
            self.next_item > self.origin && self.next_item & ((1 << self.spec.base_shift) - 1) != 0
        }

        /// Write the open nodes of every level, partial ones included, as blocks. Call it once, after the last
        /// sample.
        pub fn finish(&mut self) -> Result<(), SummaryError> {
            if self.node_open() {
                self.close_node();
            }
            self.levels.finish(self.next_item);
            self.encode()
        }

        fn tick(&self, item: u64) -> Result<i64, SummaryError> {
            let d = i64::try_from(item - self.origin)
                .map_err(|_e| SummaryError::Invalid("sample index past i64"))?;
            self.period
                .apply_rounded(d, Rounding::Floor)
                .and_then(|t| self.first_tick.checked_add(t))
                .ok_or(SummaryError::Invalid("sample tick past i64"))
        }

        fn encode(&mut self) -> Result<(), SummaryError> {
            for out in self.levels.take_output() {
                let Output {
                    level,
                    first_item,
                    items,
                    nodes,
                } = out;
                let last_tick = self.tick(first_item + items - 1)?;
                let head = SummaryHead {
                    track_id: self.track_id,
                    track_number: self.track_number,
                    tree: Tree::Series,
                    level,
                    node_shift: self.levels.shift(level),
                    layout: Layout::Series {
                        element: self.element,
                        channels: self.channels,
                        stats: self.spec.stats,
                    },
                    first_item,
                    items,
                    first_tick: self.tick(first_item)?,
                    last_tick,
                    latest_end: last_tick,
                };
                let refs: Vec<&[ChannelStats]> = nodes.iter().map(|n| n.0.as_slice()).collect();
                let mut buf = vec![0u8; crate::summary::summary_len(&head)];
                let len = write_series_summary(&mut buf, 0, &head, &refs)?;
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
        /// nodes from it on, per channel; the last may be partial (the open node).
        pub fn pending(&self, level: u8) -> (u64, Vec<Vec<ChannelStats>>) {
            let (mut first, nodes) = self.levels.pending(level);
            let mut nodes: Vec<Vec<ChannelStats>> = nodes.into_iter().map(|n| n.0).collect();
            let shift = self.spec.base_shift;
            if level == 0 && self.node_open() {
                let idx = (self.next_item - 1) >> shift;
                let from = (idx << shift).max(self.origin);
                if nodes.is_empty() {
                    first = idx;
                }
                nodes.push(
                    self.acc
                        .iter()
                        .map(|a| a.stats(self.element, self.next_item - from))
                        .collect(),
                );
            }
            (first, nodes)
        }
    }
}
