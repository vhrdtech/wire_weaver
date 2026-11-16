//! A million hostile blocks per decoder group (pieces of blocks for the positional reader) from a fixed-seed xorshift
//! stream: no panic and no allocation over the cap, but the known failures. Each stream runs as [SHARDS] tests, each
//! the cases of one slice of it, so no single test holds seconds of CPU on a loaded machine.

use crate::Times;
use crate::known::is_known;
use ww_tape_fuzz::case::{Outcome, run};
use ww_tape_fuzz::targets;

/// Inputs per decoder group: a block each.
const CASES: usize = 1_000_000;
/// Inputs of the positional reader: a piece of about 5 blocks each, so about 130,000 blocks. A piece costs about 230 us
/// in the test profile (opened as a file and as a raw medium, every entry, block and describe set read, verified,
/// walked four ways), most of it in blake3 and lz4_flex, which the test profile leaves unoptimized.
const PIECES: usize = 25_000;
/// Bytes of each input: enough for every draw of a builder.
const INPUT_LEN: usize = 96;
/// Tests per stream: shard `k` runs the cases `k * cases / SHARDS .. (k + 1) * cases / SHARDS` of the one stream.
const SHARDS: usize = 10;

/// xorshift64, from a fixed seed.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for c in buf.chunks_mut(8) {
            let x = self.next().to_le_bytes();
            c.copy_from_slice(&x[..c.len()]);
        }
    }
}

/// Run `target` on shard `shard` of [SHARDS] of the `cases` inputs of the stream seeded by `seed`: the inputs
/// before it are drawn and skipped, so every shard sees the cases it would see in one run of the whole stream.
fn stream(name: &str, target: fn(&[u8]), seed: u64, cases: usize, shard: usize) {
    let slice = shard * cases / SHARDS..(shard + 1) * cases / SHARDS;
    let mut rng = XorShift(seed);
    let mut input = [0u8; INPUT_LEN];
    let mut times = Times::default();
    let mut known = 0usize;
    // unknown failures by place: how many, the first input
    let mut failures: std::collections::BTreeMap<String, (usize, String)> = Default::default();
    for case in 0..slice.end {
        rng.fill(&mut input);
        if case < slice.start {
            continue;
        }
        // short inputs too: draws past the end are 0
        let len = INPUT_LEN - (input[INPUT_LEN - 1] as usize % 32);
        let data = &input[..len];
        let (outcome, took) = run(target, data);
        times.add(took, || format!("case {case}"));
        if outcome == Outcome::Ok {
            continue;
        }
        if is_known(name, &outcome) {
            known += 1;
            continue;
        }
        let key = match &outcome {
            Outcome::Panic(at, msg) => format!("{at}: {msg}"),
            other => format!("{other:?}"),
        };
        failures
            .entry(key)
            .or_insert_with(|| (0, data.iter().map(|b| format!("{b:02x}")).collect()))
            .0 += 1;
    }
    times.report(&format!("{name} shard {shard}"));
    eprintln!("{name} shard {shard}: {known} known failures");
    assert!(
        failures.is_empty(),
        "{name}: unknown failures (place: count, first input):\n{}",
        failures
            .iter()
            .map(|(k, (n, input))| format!("{k}: {n}, {input}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

macro_rules! streams {
    ($($test:ident: $name:literal => $target:path, $seed:literal, $cases:expr;)*) => {$(
        mod $test {
            use super::*;

            fn shard(k: usize) {
                stream($name, $target, $seed, $cases, k);
            }

            shards!(shard_0 0, shard_1 1, shard_2 2, shard_3 3, shard_4 4, shard_5 5, shard_6 6, shard_7 7, shard_8 8, shard_9 9);
        }
    )*};
}

/// One test per shard, `shard_<k>`; as many as [SHARDS].
macro_rules! shards {
    ($($test:ident $k:literal),*) => {
        const _: () = assert!([$($k),*].len() == SHARDS, "a test per shard");
        $(
            #[test]
            fn $test() {
                shard($k);
            }
        )*
    };
}

streams! {
    records_hostile_stream: "tape-records-hostile" => targets::records_hostile, 0x9e37_79b9_7f4a_7c15, CASES;
    chunk_hostile_stream: "tape-chunk-hostile" => targets::chunk_hostile, 0xbf58_476d_1ce4_e5b9, CASES;
    times_hostile_stream: "tape-times-hostile" => targets::times_hostile, 0x94d0_49bb_1331_11eb, CASES;
    payloads_hostile_stream: "tape-payloads-hostile" => targets::payloads_hostile, 0x2545_f491_4f6c_dd1d, CASES;
    samples_hostile_stream: "tape-samples-hostile" => targets::samples_hostile, 0x6a09_e667_f3bc_c908, CASES;
    series_hostile_stream: "tape-series-hostile" => targets::series_hostile, 0xbb67_ae85_84ca_a73b, CASES;
    summary_hostile_stream: "tape-summary-hostile" => targets::summary_hostile, 0x3c6e_f372_fe94_f82b, CASES;
    index_hostile_stream: "tape-index-hostile" => targets::index_hostile, 0xa54f_f53a_5f1d_36f1, CASES;
    reader_hostile_stream: "tape-reader-hostile" => targets::reader_hostile, 0x510e_527f_ade6_82d1, PIECES;
}
