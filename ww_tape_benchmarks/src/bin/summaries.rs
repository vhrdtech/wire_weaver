//! Series summaries of `ww_tape` against the samples they summarise: MB/s of sample bytes on one core, per element
//! type, through the RAW sample codec alone (no summaries), `SeriesSummaries::push` alone with every statistic, and
//! both. Prints a Markdown table with the CPU's vector extensions and the load average at the start.

use clap::Parser;
use std::hint::black_box;
use std::time::Instant;
use ww_tape::samples::{encode, max_encoded_len};
use ww_tape::summary::{Scalar, SeriesStart, SeriesStats, SeriesSummaries, SeriesSummarySpec};
use ww_tape::{Codec, Element, Magnitude, Ratio64, Sample, SampleShape, SeriesSegment};

#[derive(Parser)]
#[command(about = "Series summaries of ww_tape: MB/s per element type with and without summaries")]
struct Args {
    /// Frames handed over per push, as a driver's buffer.
    #[arg(long, default_value_t = 16 * 1024)]
    frames: usize,
    /// MiB of samples per timed pass.
    #[arg(long, default_value_t = 256)]
    mib: usize,
    /// Timed passes per case; the fastest counts.
    #[arg(long, default_value_t = 5)]
    repeats: usize,
    /// log2 of a level-0 node's samples.
    #[arg(long, default_value_t = 10)]
    base_shift: u8,
}

/// A sample type the bench generates: ADC-like noise, small steps around zero.
trait Gen: Scalar + Sample {
    fn from_noise(v: i32) -> Self;
}

macro_rules! gen_int {
    ($($t:ty),+) => {$(
        impl Gen for $t {
            fn from_noise(v: i32) -> Self {
                // the noise is a 16-bit value: truncated for 8 bits, offset for unsigned types
                let v = if <$t>::MIN == 0 { v.wrapping_add(1 << 15) } else { v };
                v as $t
            }
        }
    )+};
}
gen_int!(i8, i16, i32, i64, u8, u16, u32, u64);

impl Gen for f32 {
    fn from_noise(v: i32) -> Self {
        v as f32 / 1024.0
    }
}

impl Gen for f64 {
    fn from_noise(v: i32) -> Self {
        f64::from(v) / 1024.0
    }
}

/// `n` values of a seeded random walk in 16 bits.
fn noise<T: Gen>(n: usize) -> Vec<T> {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut acc = 0i32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            acc = (acc + (x & 0x3ff) as i32 - 512).clamp(-32768, 32767);
            T::from_noise(acc)
        })
        .collect()
}

/// What a case runs per push.
#[derive(Copy, Clone)]
enum Work {
    Raw,
    Summaries,
    Both,
}

/// MB/s of sample bytes for `work` on `mib` MiB of `batch` pushed again and again; the fastest of `repeats` passes.
fn rate<T: Gen>(args: &Args, shape: SampleShape, batch: &[T], work: Work) -> anyhow::Result<f64> {
    let frames = batch.len() / (shape.element.parts() as usize * shape.channels.0 as usize);
    let mut out = vec![
        0u8;
        max_encoded_len(Codec::RAW, shape, frames)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?
    ];
    let bytes = args.mib << 20;
    let pushes = bytes.div_ceil(size_of_val(batch));
    let spec = SeriesSummarySpec {
        base_shift: args.base_shift,
        stats: SeriesStats::all(shape.element),
        clip: None,
    };
    let start = SeriesStart {
        track_id: 1,
        track_number: 1,
        shape,
        first_item: 0,
        segment: SeriesSegment::start(0, 0, Magnitude::ONE),
        period: Ratio64::ONE,
    };
    let mut best = f64::MAX;
    for _ in 0..args.repeats {
        let mut s = SeriesSummaries::<T>::new(start, spec).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let t = Instant::now();
        for _ in 0..pushes {
            if matches!(work, Work::Raw | Work::Both) {
                black_box(
                    encode(Codec::RAW, shape, black_box(batch), &mut out)
                        .map_err(|e| anyhow::anyhow!("{e:?}"))?,
                );
            }
            if matches!(work, Work::Summaries | Work::Both) {
                s.push(black_box(batch), 0)
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                black_box(s.take_blocks());
            }
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    Ok((pushes * size_of_val(batch)) as f64 / best / 1e6)
}

fn case<T: Gen>(args: &Args, element: Element, channels: u32) -> anyhow::Result<()> {
    let shape = SampleShape::new(element, channels);
    let batch: Vec<T> = noise(args.frames * element.parts() as usize * channels as usize);
    let raw = rate(args, shape, &batch, Work::Raw)?;
    let sum = rate(args, shape, &batch, Work::Summaries)?;
    let both = rate(args, shape, &batch, Work::Both)?;
    println!("| {element:?} | {channels} | {raw:.0} | {sum:.0} | {both:.0} |");
    Ok(())
}

/// The vector extensions the summary kernels may use.
fn cpu() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        let mut v = vec!["x86-64"];
        if std::arch::is_x86_feature_detected!("avx2") {
            v.push("avx2");
        }
        if std::arch::is_x86_feature_detected!("avx512bw")
            && std::arch::is_x86_feature_detected!("avx512vl")
        {
            v.push("avx512bw+vl");
        }
        v.join(", ")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        std::env::consts::ARCH.to_owned()
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    anyhow::ensure!(
        args.frames > 0 && args.repeats > 0,
        "frames and repeats must be positive"
    );
    let load = std::fs::read_to_string("/proc/loadavg")
        .map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|_| "unknown".to_owned());
    println!(
        "Series summaries, every statistic, nodes of 2^{} samples, {} frames a push, {} MiB a pass, best of {}. CPU: {}. Load at start: {load}.",
        args.base_shift,
        args.frames,
        args.mib,
        args.repeats,
        cpu()
    );
    println!();
    println!("| element | channels | RAW MB/s | summaries MB/s | RAW + summaries MB/s |");
    println!("|---|---|---|---|---|");
    case::<i8>(&args, Element::I8, 1)?;
    case::<u8>(&args, Element::U8, 1)?;
    case::<i16>(&args, Element::I16, 1)?;
    case::<i16>(&args, Element::I16, 4)?;
    case::<u16>(&args, Element::U16, 1)?;
    case::<i32>(&args, Element::I32, 1)?;
    case::<u32>(&args, Element::U32, 1)?;
    case::<i64>(&args, Element::I64, 1)?;
    case::<u64>(&args, Element::U64, 1)?;
    case::<f32>(&args, Element::F32, 1)?;
    case::<f64>(&args, Element::F64, 1)?;
    case::<i16>(&args, Element::ComplexI16, 1)?;
    case::<i16>(&args, Element::ComplexI16, 4)?;
    case::<f32>(&args, Element::ComplexF32, 1)?;
    Ok(())
}
