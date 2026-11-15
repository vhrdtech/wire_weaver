//! The seed files in git: the ones the generator makes, within the limits on their number and size.

use std::path::Path;
use ww_tape_fuzz::seeds::{SEED_LEN_MAX, SEEDS_PER_TARGET_MAX};
use ww_tape_fuzz::targets;

/// The seed files of `target`, by name.
pub fn seed_files(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("seeds")
        .join(target);
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().into_string().unwrap(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// The seed files in git are the ones `ww_tape_fuzz_seeds write` makes: none edited by hand, none missing.
#[test]
fn seed_files_are_the_generated_ones() {
    let seeds = ww_tape_fuzz::seeds::all();
    for (target, _) in targets::ALL {
        let mut want: Vec<(String, Vec<u8>)> = seeds
            .iter()
            .filter(|s| s.target == target)
            .map(|s| (s.name.clone(), s.bytes.clone()))
            .collect();
        want.sort();
        let got = seed_files(target);
        let names = |v: &[(String, Vec<u8>)]| v.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(&got),
            names(&want),
            "{target}: run ww_tape_fuzz_seeds write"
        );
        assert!(
            got == want,
            "{target}: seed bytes differ; run ww_tape_fuzz_seeds write"
        );
    }
}

/// Every target has seeds, at most [SEEDS_PER_TARGET_MAX] files of at most [SEED_LEN_MAX] bytes.
#[test]
fn seeds_stay_few_and_small() {
    for (target, _) in targets::ALL {
        let files = seed_files(target);
        assert!(!files.is_empty(), "{target}: no seeds");
        assert!(
            files.len() <= SEEDS_PER_TARGET_MAX,
            "{target}: {} seeds",
            files.len()
        );
        for (name, bytes) in &files {
            assert!(
                bytes.len() <= SEED_LEN_MAX,
                "{target}/{name}: {} bytes",
                bytes.len()
            );
        }
    }
}
