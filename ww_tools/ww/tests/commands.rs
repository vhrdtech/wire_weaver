//! The commands the installed `ww` has: every one of `wire_weaver_cli`, `ww_std_cli`, logring's `logring_ww_cli` and
//! chunky's `chunky_ww_cli`, in help order.

use std::process::Command;

/// The names in the `Commands:` block of `ww --help`.
fn listed_commands() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_ww"))
        .arg("--help")
        .output()
        .expect("ww --help runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let help = String::from_utf8(out.stdout).expect("help is UTF-8");
    help.lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| {
            l.split_whitespace()
                .next()
                .expect("a command line")
                .to_owned()
        })
        .collect()
}

/// TOOL-36, TOOL-39: the generic commands, the standard library's, logring's and chunky's, registered from four
/// crates, make one `ww`.
#[test]
fn help_lists_every_command_in_order() {
    let expected = [
        "list",
        "usb-loopback",
        "api",
        "introspect",
        "explain",
        "observe",
        "stream",
        "monitor",
        "stats",
        "capture",
        "sync",
        "dfu",
        "chunky",
        "hub",
        "counters",
        "info",
        "report",
        "board",
        "pins",
        "elf",
        "config",
        "settings",
        #[cfg(target_os = "linux")]
        "udev",
        "logring",
        "help",
    ];
    assert_eq!(listed_commands(), expected);
}

/// A scratch directory of its own per test, empty: as `PATH`, it holds no `ww-*` plugin.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ww_own_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// `ww ARGS..` with no plugin on `PATH` and no ww.toml: what it printed, after checking it succeeded.
fn ww_alone(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ww"))
        .args(args)
        .env("PATH", dir)
        .env("WW_NO_CONFIG", "true")
        .output()
        .expect("ww runs");
    assert!(
        out.status.success(),
        "ww {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("output is UTF-8")
}

/// TOOL-39: logring's and chunky's commands are part of `ww`, with no plugin on `PATH`; `ww dfu flash` is chunky's.
#[test]
fn own_commands_need_no_plugin() {
    let dir = scratch("help");
    let logring = ww_alone(&dir, &["logring", "--help"]);
    for command in ["info", "export", "cat", "check", "make-image"] {
        assert!(logring.contains(command), "{logring}");
    }
    let chunky = ww_alone(&dir, &["chunky", "--help"]);
    for command in ["reboot", "flash", "status", "confirm", "revert"] {
        assert!(chunky.contains(command), "{chunky}");
    }
    let flash = ww_alone(&dir, &["dfu", "flash", "--help"]);
    assert!(flash.contains("--fw-version"), "{flash}");
    assert!(flash.contains("ww chunky flash"), "{flash}");
    std::fs::remove_dir_all(&dir).expect("removing the scratch directory");
}

/// `ww logring` writes an image and reads it back, as the `logring` binary does.
#[test]
fn logring_makes_and_reads_an_image() {
    let dir = scratch("logring");
    let image = dir.join("demo.wlr");
    let image = image.to_str().expect("a UTF-8 path");
    ww_alone(
        &dir,
        &[
            "logring",
            "make-image",
            image,
            "--tracks",
            "2",
            "--records",
            "20",
        ],
    );
    let info = ww_alone(&dir, &["logring", "info", image]);
    assert!(info.contains("sensor_0"), "{info}");
    ww_alone(&dir, &["logring", "check", image]);
    std::fs::remove_dir_all(&dir).expect("removing the scratch directory");
}

/// `ww dfu flash --<TAB>` completes chunky's flags from the linked command, with no plugin to ask.
#[test]
fn dfu_flash_completes_from_the_linked_command() {
    let dir = scratch("complete");
    let out = Command::new(env!("CARGO_BIN_EXE_ww"))
        .args(["--", "ww", "dfu", "flash", "--fw"])
        .env("COMPLETE", "fish")
        .env("PATH", &dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .output()
        .expect("ww completes");
    let candidates = String::from_utf8_lossy(&out.stdout);
    assert!(candidates.contains("--fw-version"), "{candidates}");
    std::fs::remove_dir_all(&dir).expect("removing the scratch directory");
}
