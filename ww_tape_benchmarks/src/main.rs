//! `ww_tape_benchmarks`: every float sample codec of `ww_tape` on simulated, decoded and sensor data, against the same
//! quantities stored as fixed-point integers with `DELTA`. Prints a Markdown table: ratio to the raw floats, and
//! encode and decode time per value on one core.

mod data;

use clap::Parser;
use data::{Columns, DataSet};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;
use ww_tape::Codec;
use ww_tape::samples::{Element, Sample, SampleShape, Samples, encode, max_encoded_len};

#[derive(Parser)]
#[command(about = "Float sample codecs of ww_tape on motor_sim, speech and ADC data")]
struct Args {
    /// The speech recording (Ogg Vorbis); `just bench-sample-codecs` fetches it. Without it the set is skipped.
    #[arg(long)]
    speech: Option<PathBuf>,
    /// Samples per chunk: each chunk is one column, coded alone.
    #[arg(long, default_value_t = 4096)]
    chunk: usize,
    /// Timed passes over each set; the fastest counts.
    #[arg(long, default_value_t = 5)]
    repeats: usize,
}

/// The float codecs, in table order after the fixed-point row.
const FLOAT_CODECS: [(Codec, &str); 5] = [
    (Codec::GORILLA, "GORILLA"),
    (Codec::CHIMP, "CHIMP"),
    (Codec::FLOAT_DELTA, "FLOAT_DELTA"),
    (Codec::FLOAT_DELTA2, "FLOAT_DELTA2"),
    (Codec::ALP, "ALP"),
];

/// One codec on one set.
struct Run {
    bytes: usize,
    encode_ns: f64,
    decode_ns: f64,
}

/// Code every chunk of `cols` and decode it back, checking every bit; the fastest of `repeats` passes.
fn run<T: Sample>(
    codec: Codec,
    shape: SampleShape,
    cols: &[Vec<T>],
    chunk: usize,
    repeats: usize,
) -> anyhow::Result<Run> {
    let lanes = shape.channels.0 as usize
        * if shape.element == Element::ComplexF32 {
            2
        } else {
            1
        };
    let pieces: Vec<&[T]> = cols.iter().flat_map(|c| c.chunks(chunk * lanes)).collect();
    let values: usize = pieces.iter().map(|p| p.len()).sum();
    let mut coded: Vec<Vec<u8>> = pieces
        .iter()
        .map(|p| Ok(vec![0u8; max_encoded_len(codec, shape, p.len() / lanes)?]))
        .collect::<Result<_, ww_tape::SampleError>>()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut lens = vec![0usize; pieces.len()];
    let mut encode_ns = f64::MAX;
    for _ in 0..repeats {
        let start = Instant::now();
        for ((p, out), len) in pieces.iter().zip(&mut coded).zip(&mut lens) {
            *len = encode(codec, shape, p, out).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        encode_ns = encode_ns.min(start.elapsed().as_nanos() as f64 / values as f64);
    }
    let mut back = vec![T::default(); chunk * lanes];
    let mut decode_ns = f64::MAX;
    for _ in 0..repeats {
        let start = Instant::now();
        for ((p, out), len) in pieces.iter().zip(&coded).zip(&lens) {
            let s = Samples::new(codec, shape, p.len() / lanes, &out[..*len])
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            s.decode(0, &mut back[..p.len()])
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            black_box(&back);
        }
        decode_ns = decode_ns.min(start.elapsed().as_nanos() as f64 / values as f64);
    }
    for ((p, out), len) in pieces.iter().zip(&coded).zip(&lens) {
        let s = Samples::new(codec, shape, p.len() / lanes, &out[..*len])
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        s.decode(0, &mut back[..p.len()])
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        anyhow::ensure!(
            back.iter()
                .zip(p.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{codec:?} didn't give the values back"
        );
    }
    Ok(Run {
        bytes: lens.iter().sum(),
        encode_ns,
        decode_ns,
    })
}

/// Values and raw bytes of the floats of a set.
fn raw(cols: &Columns) -> (usize, usize) {
    let (values, bytes) = match cols {
        Columns::F32(c) => (c.iter().map(Vec::len).sum::<usize>(), 4),
        Columns::F64(c) => (c.iter().map(Vec::len).sum::<usize>(), 8),
        Columns::I16(c) => (c.iter().map(Vec::len).sum::<usize>(), 2),
        Columns::I32(c) => (c.iter().map(Vec::len).sum::<usize>(), 4),
    };
    (values, values * bytes)
}

fn run_cols(codec: Codec, channels: u32, cols: &Columns, a: &Args) -> anyhow::Result<Run> {
    let shape = |e| SampleShape::new(e, channels);
    match cols {
        Columns::F32(c) => run(codec, shape(Element::F32), c, a.chunk, a.repeats),
        Columns::F64(c) => run(codec, shape(Element::F64), c, a.chunk, a.repeats),
        Columns::I16(c) => run(codec, shape(Element::I16), c, a.chunk, a.repeats),
        Columns::I32(c) => run(codec, shape(Element::I32), c, a.chunk, a.repeats),
    }
}

fn row(set: &str, codec: &str, raw_bytes: usize, values: usize, r: &Run) {
    println!(
        "| {set} | {codec} | {:.2} | {:.2} | {:.1} | {:.2} |",
        raw_bytes as f64 / r.bytes as f64,
        r.bytes as f64 * 8.0 / values as f64,
        r.encode_ns,
        r.decode_ns
    );
}

fn bench(set: &DataSet, a: &Args) -> anyhow::Result<()> {
    let (values, raw_bytes) = raw(&set.floats);
    let fixed = run_cols(Codec::DELTA, set.channels, &set.fixed, a)?;
    row(
        &set.name,
        &format!("DELTA of {}", set.fixed_note),
        raw_bytes,
        values,
        &fixed,
    );
    for (codec, name) in FLOAT_CODECS {
        let r = run_cols(codec, set.channels, &set.floats, a)?;
        row(&set.name, name, raw_bytes, values, &r);
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    println!(
        "sample codecs, {} samples per chunk, best of {} passes, {} kernel {:?}",
        a.chunk,
        a.repeats,
        std::env::consts::ARCH,
        ww_tape::bitpack::Kernel::best()
    );
    println!();
    println!("| Data | Codec | Ratio | Bits/value | Encode ns/value | Decode ns/value |");
    println!("|---|---|---:|---:|---:|---:|");
    let mut sets: Vec<DataSet> = data::motor().into();
    match &a.speech {
        Some(p) => sets.push(data::speech(p)?),
        None => eprintln!("speech: skipped, no --speech file"),
    }
    sets.extend(data::adc());
    for s in &sets {
        bench(s, &a)?;
    }
    Ok(())
}
