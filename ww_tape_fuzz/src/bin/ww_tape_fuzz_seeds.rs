//! The seed files of the ww_tape fuzz targets: writes them, runs one, minimises a failing input.
//!
//! ```text
//! ww_tape_fuzz_seeds write [DIR]           every seed into DIR/<target>/ (default: this crate's seeds/)
//! ww_tape_fuzz_seeds run TARGET FILE...    each file through TARGET: ok, or where it fails
//! ww_tape_fuzz_seeds minimize TARGET HEX   the shortest, plainest input that fails at the same place, in hex
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use ww_tape_fuzz::case::{Capped, minimize, run};
use ww_tape_fuzz::{seeds, targets};

#[global_allocator]
static ALLOCATOR: Capped = Capped;

fn target(name: &str) -> Result<targets::Target, String> {
    targets::by_name(name).ok_or_else(|| format!("no target {name}"))
}

fn hex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("an odd number of hex digits".to_owned());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("hex: {e}")))
        .collect()
}

fn main_inner(args: &[String]) -> Result<String, String> {
    match args {
        [cmd, rest @ ..] if cmd == "write" => {
            let dir = rest.first().map_or_else(
                || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("seeds"),
                PathBuf::from,
            );
            let n = seeds::write(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            Ok(format!("{n} seed files in {}", dir.display()))
        }
        [cmd, name, files @ ..] if cmd == "run" => {
            let t = target(name)?;
            let mut out = String::new();
            for f in files {
                let data = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
                out += &format!("{f}: {:?}\n", run(t, &data).0);
            }
            Ok(out)
        }
        [cmd, name, input] if cmd == "minimize" => {
            let t = target(name)?;
            let small = minimize(t, &hex(input)?);
            let outcome = run(t, &small).0;
            let hex: String = small.iter().map(|b| format!("{b:02x}")).collect();
            Ok(format!("{hex}\n{outcome:?}"))
        }
        _ => Err(
            "usage: ww_tape_fuzz_seeds write [DIR] | run TARGET FILE... | minimize TARGET HEX"
                .to_owned(),
        ),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match main_inner(&args) {
        Ok(out) => {
            println!("{}", out.trim_end());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
