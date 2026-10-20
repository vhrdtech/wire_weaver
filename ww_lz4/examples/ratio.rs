//! Compression ratio and host speed of the streaming encoder over captured streams, in the chunk sizes and
//! encoder sizes a firmware would use. Every block is decoded back and checked.
//!
//! `cargo run --release -p ww_lz4 --example ratio -- boot.log ls.txt top.raw`
//!
//! The ratio counts block bytes only (no framing around the blocks); speed is the encoder alone, on this PC.
use std::time::{Duration, Instant};

use ww_lz4::{Decoder, Encoder, max_block_len};

fn stream<const H: usize, const B: usize>(
    enc: &mut Encoder<H, B>,
    data: &[u8],
    chunk: usize,
) -> (usize, Duration) {
    let mut dec = Decoder::new();
    let mut blocks = Vec::new();
    let mut out = vec![0u8; max_block_len(chunk)];
    let mut spent = Duration::ZERO;
    for c in data.chunks(chunk) {
        let t = Instant::now();
        let n = enc.compress_slice(c, &mut out).unwrap();
        spent += t.elapsed();
        blocks.push(n);
        assert_eq!(dec.decode(&out[..n]).expect("decodes"), c);
    }
    (blocks.iter().sum(), spent)
}

fn report<const H: usize, const B: usize>(data: &[u8], chunk: usize, keep: usize) -> String {
    let mut enc = Box::new(Encoder::<H, B>::new(keep));
    let (total, spent) = stream(&mut enc, data, chunk);
    let mbs = data.len() as f64 / spent.as_secs_f64() / 1e6;
    format!(
        "  h{} {}K/{}K {:.2}x {mbs:4.0} MB/s",
        H.trailing_zeros(),
        B / 1024,
        keep / 1024,
        data.len() as f64 / total as f64
    )
}

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: ratio FILE..");
        std::process::exit(2);
    }
    for f in &files {
        let data = std::fs::read(f).unwrap_or_else(|e| panic!("{f}: {e}"));
        let data = &data[..data.len().min(1_000_000)];
        println!("{f} ({} B)", data.len());
        for chunk in [64, 512] {
            let mut line = format!("  chunk {chunk:3}:");
            line += &report::<1024, 2048>(data, chunk, 1024);
            line += &report::<4096, 8192>(data, chunk, 4096);
            line += &report::<4096, 12288>(data, chunk, 4096);
            line += &report::<8192, 24576>(data, chunk, 16384);
            println!("{line}");
        }
    }
}
