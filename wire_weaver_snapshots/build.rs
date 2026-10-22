//! Embeds `api_snapshots/*.ron` (copied from ww_global and ww_stdlib crates by `just save-snapshots`) as shrink_wrap
//! bytes, see `ww_self::snapshot`. The RON text is read here only: each file is encoded, the result must hash to
//! the SHA-256 `ww api save` wrote in the file's header (or the build fails), and the library embeds the bytes
//! and never parses RON.

use std::fmt::Write;
use std::path::PathBuf;
use std::{env, fs};
use ww_self::ApiBundleOwned;
use ww_self::snapshot;

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("api_snapshots");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ron"))
        .collect();
    paths.sort();

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut out = String::from("&[\n");
    for path in paths {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        let ron_text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file_name}: {e}"));
        let bundle: ApiBundleOwned =
            ron::from_str(&ron_text).unwrap_or_else(|e| panic!("{file_name}: parsing RON: {e}"));
        let bytes = snapshot::encode(&bundle).unwrap_or_else(|e| panic!("{file_name}: {e}"));
        let hash = snapshot::sha256_hex(&bytes);
        match snapshot::saved_hash(&ron_text) {
            Some(saved) if saved == hash => {}
            Some(saved) => panic!(
                "{file_name}: the RON encodes to shrink_wrap bytes with sha256 {hash}, the header says {saved}: \
                 the file was edited, or RON does not round trip; run `just save-snapshots` (--force for versions \
                 never published)"
            ),
            None => panic!(
                "{file_name} has no `{}` header line, run `just save-snapshots` (--force for versions never published)",
                snapshot::HASH_PREFIX.trim_end()
            ),
        }
        let bin_path = out_dir.join(format!("{file_name}.bin"));
        fs::write(&bin_path, &bytes).unwrap();
        writeln!(
            out,
            "    ({file_name:?}, include_bytes!({:?})),",
            bin_path.display()
        )
        .unwrap();
    }
    out.push(']');
    fs::write(out_dir.join("api_snapshots.rs"), out).unwrap();
}
