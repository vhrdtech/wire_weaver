//! Builds the firmware images in `fw/` (a method's reply as generated server code encodes it) for small CPU
//! targets with shrink_wrap's size bench runner and prints their code size, or the largest symbols of an image.
//! `--fw-features finish_for` ends the return value with `finish_for::<Wave>()` instead of `finish()`.

use anyhow::Result;
use clap::Parser;
use size_bench::{Args, Bench, Case};
use std::path::PathBuf;

/// `empty` is the image without any shrink_wrap code, what `reply` is compared with.
const CASES: &[Case] = &[
    Case {
        name: "empty",
        has_final: false,
    },
    Case {
        name: "reply",
        has_final: true,
    },
];

fn main() -> Result<()> {
    let args = Args::parse();
    let bench = Bench {
        fw: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fw"),
        bin: "size_bench_reply_fw",
        cases: CASES,
    };
    let report = bench.run(&args, &|line| eprintln!("{line}"))?;
    match args.symbols {
        Some(n) => print!("{}", report.symbols(n)),
        None => print!("{}", report.table()),
    }
    Ok(())
}
