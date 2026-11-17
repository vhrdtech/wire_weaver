//! A snapshot file saved in an older model format (`ww_self_version`) is refused by both readers, the checker's and
//! `ww_snapshots`', with the step that re-saves it; one in the current format reads. The old file is
//! `nucleo_g0b1re_api` 0.2.0 as the nucleo_g0b1re template saved it, format 0.3.0 with `#[since]` as a crate version.

use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn old_format_is_refused_with_the_re_save_step() {
    let old = manifest_dir().join("tests/fixtures/old_format/nucleo_g0b1re_api_0_2_0.ron");
    let expected = "saved in model format 0.3.0, this tool reads";
    let Err(e) = snapshot_files::read(&old) else {
        panic!("the checker read a file of format 0.3.0");
    };
    assert!(e.contains(expected), "{e}");
    assert!(e.contains("ww api save"), "{e}");
    let Err(e) = ww_snapshots::file::read(&old) else {
        panic!("ww_snapshots read a file of format 0.3.0");
    };
    let e = format!("{e:#}");
    assert!(e.contains(expected), "{e}");
    assert!(e.contains("ww api save"), "{e}");
}

#[test]
fn current_format_reads() {
    let current = manifest_dir().join("../ww_tap/api_snapshots/ww_tap_0_1_0.ron");
    snapshot_files::read(Path::new(&current)).expect("checker reads it");
    ww_snapshots::file::read(&current).expect("ww_snapshots reads it");
}
