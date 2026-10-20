//! Every block the encoder writes decodes back with lz4_flex (the earlier stream as dictionary) and with `Decoder`.

mod data;

use proptest::prelude::*;
use ww_lz4::{Decoder, Encoder, max_block_len};

/// Feeds `data` in chunks of `sizes` (cycled), checks every block, returns the total block length.
fn run<const H: usize, const B: usize>(enc: &mut Encoder<H, B>, data: &[u8], sizes: &[usize]) -> usize {
    let mut dec = Decoder::new(enc.max_chunk());
    let mut out = vec![0u8; max_block_len(enc.max_chunk())];
    let mut plain = vec![0u8; enc.max_chunk()];
    let (mut at, mut total) = (0, 0);
    for &size in sizes.iter().cycle() {
        if at >= data.len() {
            break;
        }
        let chunk = &data[at..(at + size).min(data.len())];
        let input = enc.input(chunk.len());
        assert!(input.len() >= chunk.len());
        input[..chunk.len()].copy_from_slice(chunk);
        let n = enc.compress(chunk.len(), &mut out);
        let block = &out[..n];
        assert!(n <= literal_len(chunk.len()), "a block is never longer than its literal-only form");

        let dict = &data[at.saturating_sub(ww_lz4::MAX_DISTANCE)..at];
        let m = lz4_flex::block::decompress_into_with_dict(block, &mut plain, dict).expect("lz4_flex decodes");
        assert_eq!(&plain[..m], chunk, "lz4_flex round trip at {at}");
        assert_eq!(dec.decode(block).expect("Decoder decodes"), chunk, "Decoder round trip at {at}");
        at += chunk.len();
        total += n;
        if size == 0 && sizes.iter().all(|&s| s == 0) {
            break;
        }
    }
    total
}

fn literal_len(n: usize) -> usize {
    1 + n + if n >= 15 { (n - 15) / 255 + 1 } else { 0 }
}

fn ratio(data: &[u8], total: usize) -> f64 {
    data.len() as f64 / total as f64
}

#[test]
fn text_tui_random_in_64_and_512_byte_chunks() {
    let inputs = [("text", data::text(96 * 1024, 1)), ("tui", data::tui(96 * 1024, 2)), ("random", data::random(32 * 1024, 3))];
    for (name, d) in &inputs {
        for chunk in [64, 512] {
            let small = run(&mut Encoder::<1024, 2048>::new(1024), d, &[chunk]);
            let big = run(&mut Box::new(Encoder::<4096, 12288>::new(4096)), d, &[chunk]);
            let (small, big) = (ratio(d, small), ratio(d, big));
            match *name {
                "random" => assert!(small > 0.98 && big > 0.98, "{name} {chunk}: {small:.2} {big:.2}"),
                _ => assert!(small > 1.3 && big >= small * 0.98, "{name} {chunk}: {small:.2} {big:.2}"),
            }
        }
    }
}

#[test]
fn empty_and_one_byte_chunks() {
    let mut enc = Encoder::<1024, 2048>::new(1024);
    let mut out = [0u8; max_block_len(1)];
    assert_eq!(enc.compress_slice(&[], &mut out), 1);
    assert_eq!(out[0], 0, "an empty block is one zero token");
    let mut dec = Decoder::new(enc.max_chunk());
    assert_eq!(dec.decode(&out[..1]).unwrap(), b"");
    let n = enc.compress_slice(b"x", &mut out);
    assert_eq!(&out[..n], &[0x10, b'x']);
    assert_eq!(dec.decode(&out[..n]).unwrap(), b"x");

    let d = data::text(8 * 1024, 4);
    run(&mut Encoder::<1024, 2048>::new(1024), &d, &[1]);
    run(&mut Encoder::<1024, 2048>::new(1024), &d, &[0, 1, 0, 13, 12, 14, 0, 300]);
}

#[test]
fn chunk_as_long_as_the_buffer() {
    let d = data::text(64 * 1024, 5);
    // keep 0: every chunk fills the whole buffer, so nothing reaches back
    run(&mut Encoder::<1024, 1024>::new(0), &d, &[1024]);
    // keep 1: one byte of history survives each slide
    run(&mut Encoder::<1024, 1024>::new(1), &d, &[1023]);
    let mut enc = Box::new(Encoder::<4096, 65536>::new(0));
    assert!(ratio(&d, run(&mut enc, &d, &[65536])) > 2.0);
}

#[test]
fn many_slides_of_a_small_buffer() {
    let d = data::tui(64 * 1024, 6);
    // a slide every chunk or two
    run(&mut Encoder::<256, 64>::new(32), &d, &[32, 7, 19]);
    run(&mut Encoder::<1024, 600>::new(500), &d, &[100, 64, 1]);
}

#[test]
fn history_crosses_chunks() {
    // the same line in every chunk: with history each block is a single match after the first
    let line = b"root@donguru:~# cat /proc/cpuinfo | grep MHz\r\n";
    let d: Vec<u8> = line.iter().copied().cycle().take(line.len() * 200).collect();
    let total = run(&mut Encoder::<1024, 2048>::new(1024), &d, &[line.len()]);
    assert!(ratio(&d, total) > 4.4, "{:.2}", ratio(&d, total));
}

#[test]
fn reset_starts_the_history_over() {
    let d = data::text(16 * 1024, 7);
    let (first, second) = d.split_at(8 * 1024);
    let mut enc = Encoder::<1024, 4096>::new(2048);
    let mut fresh = Encoder::<1024, 4096>::new(2048);
    let mut dec = Decoder::new(enc.max_chunk());
    let (mut a, mut b) = ([0u8; max_block_len(512)], [0u8; max_block_len(512)]);
    for c in first.chunks(512) {
        let n = enc.compress_slice(c, &mut a);
        assert_eq!(dec.decode(&a[..n]).unwrap(), c);
    }
    enc.reset();
    dec.reset();
    for (i, c) in second.chunks(512).enumerate() {
        let n = enc.compress_slice(c, &mut a);
        let m = fresh.compress_slice(c, &mut b);
        assert_eq!(&a[..n], &b[..m], "after reset the blocks depend on the bytes since alone");
        let mut plain = [0u8; 512];
        let k = lz4_flex::block::decompress_into_with_dict(&a[..n], &mut plain, &second[..i * 512]).unwrap();
        assert_eq!(&plain[..k], c);
        assert_eq!(dec.decode(&a[..n]).unwrap(), c);
    }
}

#[test]
fn backs_off_on_random_and_comes_back_on_text() {
    let noise = data::random(16 * 1024, 8);
    let text = data::text(16 * 1024, 9);
    let mut enc = Encoder::<1024, 2048>::new(1024).with_backoff(2, 4096);
    let mut out = [0u8; max_block_len(512)];
    let mut dec = Decoder::new(enc.max_chunk());
    for c in noise.chunks(512) {
        let n = enc.compress_slice(c, &mut out);
        assert_eq!(n, literal_len(c.len()), "random goes out as literals");
        assert_eq!(dec.decode(&out[..n]).unwrap(), c);
    }
    // backed off: the first 4 KiB of text (8 chunks) at most go as literals, then it compresses again
    let mut sizes = vec![];
    for c in text.chunks(512) {
        let n = enc.compress_slice(c, &mut out);
        assert_eq!(dec.decode(&out[..n]).unwrap(), c);
        sizes.push(n);
    }
    assert!(sizes[..9].iter().any(|&n| n < 400), "{sizes:?}");
    assert!(sizes[9..].iter().all(|&n| n < 400), "{sizes:?}");
}

#[test]
fn never_backing_off_still_sends_literals() {
    let noise = data::random(8 * 1024, 10);
    let mut enc = Encoder::<1024, 2048>::new(1024).with_backoff(0, 0);
    let total = run(&mut enc, &noise, &[512]);
    assert_eq!(total, 16 * literal_len(512));
}

#[test]
fn decoder_errors() {
    let mut dec = Decoder::new(16);
    // a literal run of 20 bytes is longer than max_chunk
    let mut long = vec![0xf0, 5];
    long.extend_from_slice(&[b'a'; 20]);
    assert_eq!(dec.decode(&long), Err(ww_lz4::DecodeError::TooLong));
    // a match reaching back before the start of the stream
    assert_eq!(dec.decode(&[0x10, b'a', 9, 0, 0x00]), Err(ww_lz4::DecodeError::Corrupt));
    // the history is unchanged after an error
    assert_eq!(dec.decode(&[0x20, b'o', b'k']).unwrap(), b"ok");
}

#[test]
fn decoder_keeps_64_kib_of_history() {
    // a match 65535 back, after the decoder trimmed its history more than once
    let d = data::random(300 * 1024, 11);
    let mut dec = Decoder::new(65536);
    let mut at = 0usize;
    for c in d.chunks(65536) {
        let mut block = vec![0u8; lz4_flex::block::get_maximum_output_size(c.len())];
        let n = lz4_flex::block::compress_into_with_dict(c, &mut block, &d[at.saturating_sub(65535)..at]).unwrap();
        assert_eq!(dec.decode(&block[..n]).unwrap(), c);
        at += c.len();
    }
    // 16 literals, a match of 5 at offset 65535, then 5 last literals
    let mut block = vec![0xf1, 1];
    block.extend_from_slice(&[0; 16]);
    block.extend_from_slice(&[0xff, 0xff, 0x50]);
    block.extend_from_slice(b"12345");
    let got = dec.decode(&block).unwrap().to_vec();
    let src = d.len() + 16 - 65535;
    assert_eq!(&got[..16], &[0; 16]);
    assert_eq!(&got[16..21], &d[src..src + 5], "reached 65535 back");
    assert_eq!(&got[21..], b"12345");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn random_chunking(
        seed in any::<u64>(),
        kind in 0..3u8,
        len in 0..6000usize,
        sizes in prop::collection::vec(0..=512usize, 1..20),
    ) {
        let d = match kind { 0 => data::text(len, seed), 1 => data::tui(len, seed), _ => data::random(len, seed) };
        let mut sizes = sizes;
        if sizes.iter().all(|&s| s == 0) {
            sizes.push(1);
        }
        run(&mut Encoder::<256, 1024>::new(512), &d, &sizes);
    }
}
