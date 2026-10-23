//! `ww --rtt` device selection (TOOL-20), up to the point where a probe would be opened: no probe needed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ww(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ww"));
    cmd.args(args)
        .current_dir(dir)
        .env("WW_RUNTIME_DIR", dir)
        .env("NO_COLOR", "1");
    for (key, _) in std::env::vars() {
        if key.starts_with("WW_") && key != "WW_RUNTIME_DIR" {
            cmd.env_remove(key);
        }
    }
    cmd.output().expect("run ww")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-cli-rtt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A board project's ww.toml names the chip, probe and ELF; `ww config show` says where each comes from.
#[test]
fn rtt_settings_from_ww_toml() {
    let dir = temp_dir("toml");
    std::fs::write(
        dir.join("ww.toml"),
        "[device]\nrtt = \"STM32G0B1RETx\"\nprobe = \"0483:374b\"\nelf = \"target/fw.elf\"\nprobe_speed_khz = 4000\n",
    )
    .unwrap();
    let show = ww(&dir, &["config", "show"]);
    assert!(show.status.success(), "{}", text(&show));
    let out = text(&show);
    for line in [
        "rtt",
        "STM32G0B1RETx",
        "probe",
        "0483:374b",
        "target/fw.elf",
        "4000",
    ] {
        assert!(out.contains(line), "{line} in {out}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A USB filter on the command line next to `--rtt` would select a different device: refused before anything is
/// opened, and so is a probe setting without `--rtt`.
#[test]
fn contradicting_selection_is_refused() {
    let dir = temp_dir("refused");
    let both = ww(&dir, &["--rtt", "STM32G0B1RETx", "--serial", "0B1", "info"]);
    assert!(!both.status.success());
    assert!(
        text(&both).contains("selects a USB device"),
        "{}",
        text(&both)
    );

    let probe_only = ww(&dir, &["--probe", "066D", "counters", "read"]);
    assert!(!probe_only.status.success());
    assert!(
        text(&probe_only).contains("needs rtt"),
        "{}",
        text(&probe_only)
    );

    let sync_device = ww(&dir, &["--rtt", "STM32G0B1RETx", "sync", "0B1"]);
    assert!(!sync_device.status.success());
    let _ = std::fs::remove_dir_all(dir);
}

/// `--rtt` from the command line wins over ww.toml's USB filters, which are left out instead of failing.
#[test]
fn rtt_flag_over_usb_ww_toml() {
    let dir = temp_dir("over");
    std::fs::write(dir.join("ww.toml"), "[device]\nserial = \"0B1\"\n").unwrap();
    // gets as far as looking for the probe
    let info = ww(
        &dir,
        &["--rtt", "STM32G0B1RETx", "--probe", "ffff:ffff", "info"],
    );
    assert!(!info.status.success());
    let out = text(&info);
    assert!(out.contains("not used over RTT: serial"), "{out}");
    assert!(out.contains("probe USB VID:PID = ffff:ffff"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}
