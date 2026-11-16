//! The kernels against the summaries' earlier code, kept here as the model: the same bits for every scalar type,
//! kernel, channel count, batch split, statistic subset and clip range.

use super::{IntBatch, Kernel, Narrow, int_batch};
use crate::Element;
use crate::summary::series::{Acc, Channel, add_channel};
use crate::summary::{ChannelStats, Scalar, SeriesStats, Value};

/// xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The earlier statistics of one component: one pass per statistic, sums in i128.
#[derive(Copy, Clone, Debug)]
struct Part<T> {
    min: Option<T>,
    max: Option<T>,
    isum: i128,
    fsum: f64,
    isq: i128,
    fsq: f64,
}

/// The earlier statistics of one channel.
#[derive(Copy, Clone, Debug)]
struct Model<T> {
    parts: [Part<T>; 2],
    ipow: i128,
    fpow: f64,
    pmax: f64,
    clipped: u64,
}

fn fmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b > a { b } else { a }
}

impl<T: Scalar> Model<T> {
    const NEW: Self = Model {
        parts: [Part {
            min: None,
            max: None,
            isum: 0,
            fsum: 0.0,
            isq: 0,
            fsq: 0.0,
        }; 2],
        ipow: 0,
        fpow: 0.0,
        pmax: f64::NAN,
        clipped: 0,
    };

    /// The earlier `SeriesSummaries::add` for one channel.
    fn add(&mut self, xs: &[T], ch: Channel<T>) {
        let comps = if ch.complex { 2 } else { 1 };
        let (stride, at, stats) = (ch.stride, ch.at, ch.stats);
        let xs_at = xs.get(at..).unwrap_or_default();
        for (c, p) in self.parts.iter_mut().take(comps).enumerate() {
            if stats.min || stats.max {
                let (mut lo, mut hi) = (p.min, p.max);
                for &x in xs_at.iter().skip(c).step_by(stride) {
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
                (p.min, p.max) = (lo, hi);
            }
            if stats.sum || stats.sum_sq {
                if T::INT {
                    let (mut s, mut q) = (0i128, 0i128);
                    let mut fq = 0.0f64;
                    for &x in xs_at.iter().skip(c).step_by(stride) {
                        let v = x.to_i128();
                        s += v;
                        if T::SQ_EXACT {
                            q += v * v;
                        } else {
                            let f = x.to_f64();
                            fq += f * f;
                        }
                    }
                    p.isum += s;
                    p.isq += q;
                    p.fsq += fq;
                } else {
                    for &x in xs_at.iter().skip(c).step_by(stride) {
                        let f = x.to_f64();
                        p.fsum += f;
                        p.fsq += f * f;
                    }
                }
            }
        }
        if ch.complex && (stats.power_mean || stats.power_max) {
            for frame in xs.chunks_exact(stride) {
                let (i, q) = (frame[at], frame[at + 1]);
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
        if stats.clipped {
            for frame in xs.chunks_exact(stride) {
                if frame[at..at + comps]
                    .iter()
                    .any(|&x| !(ch.lo < x && x < ch.hi))
                {
                    self.clipped += 1;
                }
            }
        }
    }

    /// The earlier `Acc::stats`.
    fn stats(&self, element: Element, samples: u64) -> ChannelStats {
        let mut s = ChannelStats::empty(element);
        let comps = if element.parts() == 2 { 2 } else { 1 };
        for (c, p) in self.parts.iter().take(comps).enumerate() {
            if let Some(v) = p.min {
                s.min[c] = v.value();
            }
            if let Some(v) = p.max {
                s.max[c] = v.value();
            }
            if T::INT {
                s.sum[c] = p.isum as f64;
                s.sum_sq[c] = if T::SQ_EXACT { p.isq as f64 } else { p.fsq };
            } else {
                s.sum[c] = p.fsum;
                s.sum_sq[c] = p.fsq;
            }
        }
        s.clipped = self.clipped;
        if element.parts() == 2 && samples > 0 {
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

/// The bits of `v`, every NaN as one: Rust leaves a NaN result's sign and payload to the compiler.
fn bits(v: f64) -> u64 {
    if v.is_nan() {
        f64::NAN.to_bits()
    } else {
        v.to_bits()
    }
}

fn same_bits(a: &ChannelStats, b: &ChannelStats) -> bool {
    let value = |v: Value| match v {
        Value::F64(f) => bits(f),
        v => v.to_bits(),
    };
    let single = |v: f32| bits(f64::from(v));
    a.min.map(value) == b.min.map(value)
        && a.max.map(value) == b.max.map(value)
        && a.sum.map(bits) == b.sum.map(bits)
        && a.sum_sq.map(bits) == b.sum_sq.map(bits)
        && a.clipped == b.clipped
        && a.flags == b.flags
        && single(a.power_mean) == single(b.power_mean)
        && single(a.power_max) == single(b.power_max)
}

/// Statistic subsets: every one, none of the sums, only power, only clipping.
fn subsets(element: Element) -> [SeriesStats; 4] {
    let all = SeriesStats::all(element);
    [
        all,
        SeriesStats {
            sum: false,
            sum_sq: false,
            ..all
        },
        SeriesStats {
            power_mean: all.power_mean,
            power_max: all.power_max,
            min: true,
            ..SeriesStats::default()
        },
        SeriesStats {
            clipped: true,
            ..SeriesStats::default()
        },
    ]
}

/// Every kernel and the model give the same bits on `data` (frames of `channels` channels of `element`), pushed in
/// batches of random lengths, for each statistic subset and both clip ranges.
fn check<T: Scalar>(
    element: Element,
    channels: usize,
    data: &[T],
    clips: [(T, T); 2],
    rng: &mut Rng,
) {
    let comps = if element.parts() == 2 { 2 } else { 1 };
    let stride = channels * comps;
    let frames = data.len() / stride;
    // batch ends: at most a node of 2^24, here a few thousand frames
    let mut ends = vec![0];
    while *ends.last().unwrap() < frames {
        let n = match rng.below(4) {
            0 => 1,
            1 => rng.below(70) as usize,
            _ => rng.below(3000) as usize,
        };
        ends.push((ends.last().unwrap() + n).min(frames));
    }
    for stats in subsets(element) {
        for (lo, hi) in clips {
            for kernel in Kernel::all() {
                for ch in 0..channels {
                    let c = Channel {
                        stride,
                        at: ch * comps,
                        complex: comps == 2,
                        lo,
                        hi,
                        stats,
                    };
                    let (mut acc, mut model) = (Acc::NEW, Model::NEW);
                    for w in ends.windows(2) {
                        let xs = &data[w[0] * stride..w[1] * stride];
                        add_channel(&mut acc, xs, c, kernel);
                        model.add(xs, c);
                    }
                    let (got, want) = (
                        acc.stats(element, frames as u64),
                        model.stats(element, frames as u64),
                    );
                    assert!(
                        same_bits(&got, &want),
                        "{element:?} {channels} ch {ch} {kernel:?} {stats:?}: {got:?} vs {want:?}"
                    );
                }
            }
        }
    }
}

/// Values of an integer type: mostly random, with its ends and runs near them.
fn ints<T: Copy>(n: usize, rng: &mut Rng, from: impl Fn(u64) -> T, ends: [T; 2]) -> Vec<T> {
    (0..n)
        .map(|_| match rng.below(16) {
            0 => ends[0],
            1 => ends[1],
            _ => from(rng.next()),
        })
        .collect()
}

#[test]
fn integers_match_the_earlier_code() {
    let mut rng = Rng(7);
    for channels in [1, 2, 3] {
        let n = 4100 * channels;
        let d = ints(n, &mut rng, |v| v as i8, [i8::MIN, i8::MAX]);
        check(
            Element::I8,
            channels,
            &d,
            [(i8::MIN, i8::MAX), (-100, 90)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v as u8, [u8::MIN, u8::MAX]);
        check(
            Element::U8,
            channels,
            &d,
            [(u8::MIN, u8::MAX), (10, 251)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v as i16, [i16::MIN, i16::MAX]);
        check(
            Element::I16,
            channels,
            &d,
            [(i16::MIN, i16::MAX), (-3000, 3000)],
            &mut rng,
        );
        let d = ints(2 * n, &mut rng, |v| v as i16, [i16::MIN, i16::MAX]);
        check(
            Element::ComplexI16,
            channels,
            &d,
            [(i16::MIN, i16::MAX), (-3000, 3000)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v as u16, [u16::MIN, u16::MAX]);
        check(
            Element::U16,
            channels,
            &d,
            [(u16::MIN, u16::MAX), (1, 60_000)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v as i32, [i32::MIN, i32::MAX]);
        check(
            Element::I32,
            channels,
            &d,
            [(i32::MIN, i32::MAX), (-1 << 30, 1 << 29)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v as u32, [u32::MIN, u32::MAX]);
        check(
            Element::U32,
            channels,
            &d,
            [(u32::MIN, u32::MAX), (5, 1 << 31)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v as i64, [i64::MIN, i64::MAX]);
        check(
            Element::I64,
            channels,
            &d,
            [(i64::MIN, i64::MAX), (-1 << 62, 1 << 40)],
            &mut rng,
        );
        let d = ints(n, &mut rng, |v| v, [u64::MIN, u64::MAX]);
        check(
            Element::U64,
            channels,
            &d,
            [(u64::MIN, u64::MAX), (3, 1 << 63)],
            &mut rng,
        );
    }
}

#[test]
fn floats_match_the_earlier_code() {
    let mut rng = Rng(11);
    let special = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        1e300,
        -1e-300,
    ];
    for channels in [1, 2, 3] {
        let n = 4100 * channels;
        let d: Vec<f64> = (0..n)
            .map(|_| match rng.below(12) {
                0 => special[rng.below(special.len() as u64) as usize],
                _ => (rng.next() >> 11) as f64 / 3.7 - 1e15,
            })
            .collect();
        check(
            Element::F64,
            channels,
            &d,
            [(f64::NEG_INFINITY, f64::INFINITY), (-1e14, 1e14)],
            &mut rng,
        );
        let f: Vec<f32> = d.iter().map(|&v| (v / 1e9) as f32).collect();
        check(
            Element::F32,
            channels,
            &f,
            [(f32::NEG_INFINITY, f32::INFINITY), (-1e5, 1e5)],
            &mut rng,
        );
        let f: Vec<f32> = d.iter().chain(&d).map(|&v| (v / 1e9) as f32).collect();
        check(
            Element::ComplexF32,
            channels,
            &f,
            [(f32::NEG_INFINITY, f32::INFINITY), (-1e5, 1e5)],
            &mut rng,
        );
    }
    // a channel of only NaN
    let nan = vec![f32::NAN; 300];
    check(
        Element::F32,
        1,
        &nan,
        [(f32::NEG_INFINITY, f32::INFINITY); 2],
        &mut rng,
    );
}

/// [int_batch] against i128 arithmetic, for complex frames of every narrow type (the series take complex i16 only).
fn batch_exact<T: Narrow + Into<i128> + core::fmt::Debug, const C: usize>(
    frames: &[[T; C]],
    lo: T,
    hi: T,
) {
    let comps: Vec<Vec<i128>> = (0..C)
        .map(|c| frames.iter().map(|f| f[c].into()).collect())
        .collect();
    for kernel in Kernel::all() {
        let mut got = IntBatch::NEW;
        kernel.int_batch(frames, lo, hi, &mut got);
        assert_eq!(got.frames, frames.len() as u64, "{kernel:?}");
        for c in 0..C {
            assert_eq!(got.sum[c], comps[c].iter().sum::<i128>(), "{kernel:?}");
            assert_eq!(
                got.sq[c],
                comps[c].iter().map(|v| v * v).sum::<i128>(),
                "{kernel:?}"
            );
            assert_eq!(
                Some(got.min[c]),
                frames.iter().map(|f| f[c]).min(),
                "{kernel:?}"
            );
            assert_eq!(
                Some(got.max[c]),
                frames.iter().map(|f| f[c]).max(),
                "{kernel:?}"
            );
        }
        let clipped = frames
            .iter()
            .filter(|f| f.iter().any(|&x| x <= lo || x >= hi))
            .count();
        assert_eq!(got.clipped, clipped as u64, "{kernel:?}");
        if C == 2 {
            let p: Vec<i128> = (0..frames.len())
                .map(|k| comps[0][k] * comps[0][k] + comps[1][k] * comps[1][k])
                .collect();
            assert_eq!(got.pow, p.iter().sum::<i128>(), "{kernel:?}");
            assert_eq!(got.pmax, p.iter().copied().max().unwrap(), "{kernel:?}");
        }
    }
    // the portable code is the same in one batch or many
    let mut one = IntBatch::NEW;
    int_batch(frames, lo, hi, &mut one);
    let mut split = IntBatch::NEW;
    for f in frames.chunks(37) {
        int_batch(f, lo, hi, &mut split);
    }
    assert_eq!(one, split);
}

#[test]
fn batches_are_exact_at_the_ends_of_every_type() {
    let mut rng = Rng(3);
    macro_rules! each {
        ($($t:ty),+) => {$(
            let ends = [<$t>::MIN, <$t>::MAX];
            // every frame at an end: the largest sums, squares and powers
            let edge: Vec<[$t; 2]> = (0..1000).map(|k| [ends[k % 2], ends[(k / 2) % 2]]).collect();
            batch_exact(&edge, <$t>::MIN, <$t>::MAX);
            let rand: Vec<[$t; 2]> = (0..3001).map(|_| [rng.next() as $t, rng.next() as $t]).collect();
            batch_exact(&rand, <$t>::MIN, <$t>::MAX);
            let one: Vec<[$t; 1]> = rand.iter().map(|f| [f[0]]).collect();
            batch_exact(&one, <$t>::MIN / 2 + 1, <$t>::MAX / 3);
        )+};
    }
    each!(i8, u8, i16, u16, i32, u32);
}

/// The summary blocks are the same bytes whichever kernel computed them.
#[test]
fn blocks_are_the_same_with_every_kernel() {
    use crate::summary::{SeriesStart, SeriesSummaries, SeriesSummarySpec};
    use crate::{Magnitude, Ratio64, SampleShape, SeriesSegment};
    let mut rng = Rng(5);
    let data: Vec<i16> = (0..2 * 3 * 20_000).map(|_| rng.next() as i16).collect();
    let shape = SampleShape::new(Element::ComplexI16, 3);
    let blocks = |kernel: Kernel| {
        let start = SeriesStart {
            track_id: 9,
            track_number: 2,
            shape,
            first_item: 100,
            segment: SeriesSegment::start(0, 0, Magnitude::ONE),
            period: Ratio64::ONE,
        };
        let spec = SeriesSummarySpec {
            base_shift: 6,
            stats: SeriesStats::all(shape.element),
            clip: None,
        };
        let mut s = SeriesSummaries::<i16>::new(start, spec).unwrap();
        s.set_kernel(kernel);
        for part in data.chunks(6 * 777) {
            s.push(part, 1).unwrap();
        }
        s.finish().unwrap();
        s.take_blocks()
    };
    let want = blocks(Kernel::Portable);
    assert!(!want.is_empty());
    for kernel in Kernel::all() {
        assert_eq!(blocks(kernel), want, "{kernel:?}");
    }
}
