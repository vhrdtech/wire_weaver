//! `WW_API_SAVE=1 cargo test --test evolution` saves the snapshot from an API crate's own test. A test binary of its
//! own, as it sets the variable for the whole process.

use wire_weaver_evolution::snapshot_files::SNAPSHOTS_DIR;
use wire_weaver_evolution::{Check, SAVE_ENV};

#[test]
fn assert_saves_with_env() {
    let dir = std::env::temp_dir()
        .join("ww_evolution_save_env_test")
        .join(std::process::id().to_string());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"test_api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "use wire_weaver::prelude::*;\n#[ww_api_root]\npub trait Api {\n    fn led_on();\n}\n",
    )
    .unwrap();

    let check = Check::new(&dir).require_current(true);
    assert!(check.run().unwrap().verdict().is_err());
    // SAFETY: the only test in this binary, no other thread reads the environment
    unsafe { std::env::set_var(SAVE_ENV, "1") };
    check.assert();
    assert!(dir.join(SNAPSHOTS_DIR).join("test_api_0_1_0.ron").exists());
    unsafe { std::env::remove_var(SAVE_ENV) };
    check.assert();
    let _ = std::fs::remove_dir_all(&dir);
}
