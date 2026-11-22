//! `WW_API_SAVE=1 cargo test --test evolution` saves the snapshot from an API crate's own test. The variable is set
//! for a child process of this test binary, which runs the test again.

use wire_weaver_evolution::snapshot_files::SNAPSHOTS_DIR;
use wire_weaver_evolution::{Check, SAVE_ENV};

/// Set in the child process the test starts: the API crate's directory.
const CHILD_DIR_ENV: &str = "WW_EVOLUTION_SAVE_ENV_CHILD_DIR";

#[test]
fn assert_saves_with_env() {
    // the child: `assert` with SAVE_ENV set in its environment saves the snapshot
    if let Some(dir) = std::env::var_os(CHILD_DIR_ENV) {
        Check::new(std::path::Path::new(&dir))
            .require_current(true)
            .assert();
        return;
    }
    let dir = std::env::temp_dir()
        .join("ww_evolution_save_env_test")
        .join(std::process::id().to_string());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"test_api\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "use ww_api::prelude::*;\n#[ww_api_root] #[effect(edit)]\npub trait Api {\n    fn led_on();\n}\n",
    )
    .unwrap();

    let check = Check::new(&dir).require_current(true);
    assert!(check.run().unwrap().verdict().is_err());
    // the variable is set in a child process of this test binary: setting it in this one is unsafe code
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["assert_saves_with_env", "--exact", "--nocapture"])
        .env(CHILD_DIR_ENV, &dir)
        .env(SAVE_ENV, "1")
        .status()
        .unwrap();
    assert!(status.success(), "the child process failed: {status}");
    assert!(dir.join(SNAPSHOTS_DIR).join("test_api_0_1_0.ron").exists());
    check.assert();
    let _ = std::fs::remove_dir_all(&dir);
}
