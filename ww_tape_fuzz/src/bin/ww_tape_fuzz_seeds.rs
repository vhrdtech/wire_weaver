//! The seed files of the ww_tape fuzz targets: writes them, runs one, minimises a failing input.
//!
//! ```text
//! ww_tape_fuzz_seeds write [DIR]           every seed into DIR/<target>/ (default: this crate's seeds/)
//! ww_tape_fuzz_seeds run TARGET PATH...    each file (a directory: its files) through TARGET: failures by place
//! ww_tape_fuzz_seeds minimize TARGET HEX   the shortest, plainest input that fails at the same place, in hex
//! ```

use std::collections::BTreeMap;
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

/// The files `paths` name: a file itself, a directory its files, in name order.
fn files(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut out = vec![];
    for p in paths {
        let p = PathBuf::from(p);
        if p.is_dir() {
            let rd = std::fs::read_dir(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let mut in_dir = rd
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{}: {e}", p.display()))?;
            in_dir.sort();
            out.extend(in_dir);
        } else {
            out.push(p);
        }
    }
    Ok(out)
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
        [cmd, name, paths @ ..] if cmd == "run" => {
            let t = target(name)?;
            // by place: how many files fail there, the first of them and its message
            let mut places: BTreeMap<String, (usize, String, String)> = BTreeMap::new();
            let mut passed = 0usize;
            for f in files(paths)? {
                let data = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
                let outcome = run(t, &data).0;
                let Some(place) = outcome.place().map(str::to_owned) else {
                    passed += 1;
                    continue;
                };
                let first = || (0, f.display().to_string(), format!("{outcome:?}"));
                places.entry(place).or_insert_with(first).0 += 1;
            }
            let mut out = format!("{passed} passed\n");
            for (place, (n, first, outcome)) in &places {
                out += &format!("{place}: {n} files, first {first}\n  {outcome}\n");
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
            "usage: ww_tape_fuzz_seeds write [DIR] | run TARGET PATH... | minimize TARGET HEX"
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
