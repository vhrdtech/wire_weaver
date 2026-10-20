//! Blocks of a few streams for the cross-check against C liblz4 (`just test-lz4-c`, `tests/liblz4/check.py`).
//!
//! One line per block, `<block hex> <plain hex>`; `# name` starts a stream, `reset` marks an `Encoder::reset`.
#[path = "../tests/data/mod.rs"]
mod data;

use std::fmt::Write;

use ww_lz4::{Encoder, max_block_len};

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::with_capacity(2 * b.len()), |mut s, x| {
        write!(s, "{x:02x}").unwrap();
        s
    })
}

/// Prints the blocks of `data` cut in chunks of `sizes` (cycled), with a reset before the chunk at `reset_at`.
fn stream<const H: usize, const B: usize>(name: &str, mut enc: Box<Encoder<H, B>>, data: &[u8], sizes: &[usize], reset_at: Option<usize>) {
    println!("# {name}");
    let mut out = vec![0u8; max_block_len(enc.max_chunk())];
    let mut at = 0;
    for &size in sizes.iter().cycle() {
        if at >= data.len() {
            break;
        }
        if reset_at == Some(at) {
            enc.reset();
            println!("reset");
        }
        let chunk = &data[at..(at + size).min(data.len())];
        let n = enc.compress_slice(chunk, &mut out);
        println!("{} {}", hex(&out[..n]), hex(chunk));
        at += chunk.len();
    }
}

fn main() {
    stream("text 512 h12 12K/4K", Box::new(Encoder::<4096, 12288>::new(4096)), &data::text(48 * 1024, 1), &[512], None);
    stream("tui 64 h10 2K/1K", Box::new(Encoder::<1024, 2048>::new(1024)), &data::tui(24 * 1024, 2), &[64], None);
    let mixed = [data::random(8 * 1024, 3), data::text(8 * 1024, 4)].concat();
    stream("random then text", Box::new(Encoder::<1024, 2048>::new(1024)), &mixed, &[512], None);
    stream("odd chunks, small buffer", Box::new(Encoder::<256, 600>::new(500)), &data::tui(16 * 1024, 5), &[0, 1, 13, 100, 37], None);
    stream("chunk = buffer", Box::new(Encoder::<1024, 1024>::new(0)), &data::text(8 * 1024, 6), &[1024], None);
    stream("reset in the middle", Box::new(Encoder::<1024, 4096>::new(2048)), &data::text(16 * 1024, 7), &[512], Some(8 * 1024));
    stream("64 KiB buffer", Box::new(Encoder::<4096, 65536>::new(32768)), &data::text(160 * 1024, 8), &[4096], None);
}
