//! Embeds `api_snapshots/*.ron` (copied from ww_global and ww_stdlib crates by `just save-snapshots`).

use std::fmt::Write;
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("api_snapshots");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ron"))
        .collect();
    paths.sort();

    let mut out = String::from("&[\n");
    for path in paths {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        writeln!(
            out,
            "    ({file_name:?}, include_str!({:?})),",
            path.display()
        )
        .unwrap();
    }
    out.push(']');
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("api_snapshots.rs");
    fs::write(out_path, out).unwrap();
}
