//! The hostile builders: the same input gives the same bytes, and the CRCs are good often enough for the decoders'
//! deeper paths to run.

use ww_tape::{Block, block_len};
use ww_tape_fuzz::hostile;

/// A hostile builder: input to block or file.
type Builder = fn(&[u8]) -> Vec<u8>;

const BUILDERS: [(&str, Builder); 9] = [
    ("records", hostile::records),
    ("chunk", hostile::chunk),
    ("times", hostile::times),
    ("payloads", hostile::payloads),
    ("samples", hostile::samples),
    ("series", hostile::series),
    ("summary", hostile::summary),
    ("index", hostile::index),
    ("file", hostile::file),
];

/// Inputs from a fixed xorshift stream.
fn inputs(n: usize) -> Vec<Vec<u8>> {
    let mut x = 0x2545_f491_4f6c_dd1d_u64;
    (0..n)
        .map(|i| {
            (0..64 + i % 32)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect()
        })
        .collect()
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    for (name, build) in BUILDERS {
        for input in inputs(200) {
            assert_eq!(build(&input), build(&input), "{name}");
        }
    }
}

#[test]
fn every_builder_takes_any_input() {
    for (_, build) in BUILDERS {
        for input in [&[][..], &[0xff; 3], &[0; 200]] {
            build(input);
        }
    }
}

/// Of 1000 blocks, most carry a good block CRC and some read whole: hostile values reach the decoders behind the CRCs.
#[test]
fn most_blocks_pass_their_crcs() {
    for (name, build) in BUILDERS {
        let (mut crc_good, mut good) = (0, 0);
        for input in inputs(1000) {
            let bytes = build(&input);
            let first = block_len(&bytes).ok().and_then(|n| bytes.get(..n));
            let Some(first) = first else { continue };
            let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
            let (covered, stored) = first.split_at(first.len() - 4);
            if crc.checksum(covered).to_le_bytes() == stored {
                crc_good += 1;
            }
            if Block::read(first).is_ok() {
                good += 1;
            }
        }
        assert!(crc_good > 700, "{name}: {crc_good} of 1000 with a good CRC");
        assert!(good > 50, "{name}: {good} of 1000 read whole");
    }
}

/// Every block a builder makes is at most 1 KiB, so a million of them decode in seconds.
#[test]
fn blocks_stay_small() {
    for (name, build) in BUILDERS {
        for input in inputs(2000) {
            let bytes = build(&input);
            let mut rest = &bytes[..];
            while let Ok(n) = block_len(rest) {
                let Some((_, after)) = rest.split_at_checked(n) else {
                    break;
                };
                assert!(n <= 1024, "{name}: a block of {n} bytes");
                rest = after;
            }
        }
    }
}
