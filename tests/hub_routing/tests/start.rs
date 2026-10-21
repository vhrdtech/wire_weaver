//! Clients starting the hub (CLT-39), as separate processes: `hub_host` is both the client and the hub executable.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const HUB_HOST: &str = env!("CARGO_BIN_EXE_hub_host");

fn runtime_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-hub-routing-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn hub_host(dir: &Path, arg: &str) -> Command {
    let mut cmd = Command::new(HUB_HOST);
    cmd.arg(arg)
        .env("WW_RUNTIME_DIR", dir)
        .env("WW_HUB_EXE", HUB_HOST)
        // a hub a failed test leaves behind goes soon
        .env("WW_HUB_IDLE_EXIT", "5");
    cmd
}

fn pid_of(out: &Output) -> u32 {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn stop(dir: &Path) {
    let out = hub_host(dir, "stop").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // it lets go of hub.lock as it exits
    let deadline = Instant::now() + Duration::from_secs(2);
    let lock = dir.join("ww").join("hub.lock");
    while Instant::now() < deadline {
        let held = std::fs::File::open(&lock)
            .is_ok_and(|f| matches!(f.try_lock_shared(), Err(std::fs::TryLockError::WouldBlock)));
        if !held {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the hub didn't exit");
}

#[test]
fn two_programs_starting_a_hub_at_once_get_one() {
    let dir = runtime_dir("race");
    let spawn = || {
        hub_host(&dir, "ensure")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let (a, b) = (spawn(), spawn());
    let (a, b) = (a.wait_with_output().unwrap(), b.wait_with_output().unwrap());
    let (a, b) = (pid_of(&a), pid_of(&b));
    assert_eq!(a, b, "both found the same hub");
    // a third one finds it too, without starting anything
    let c = pid_of(&hub_host(&dir, "ensure").output().unwrap());
    assert_eq!(a, c);
    stop(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stale_socket_is_replaced() {
    let dir = runtime_dir("stale");
    let ww = dir.join("ww");
    std::fs::create_dir_all(&ww).unwrap();
    // left behind by a hub that crashed: a plain file where the socket goes, hub.lock not held
    std::fs::write(ww.join("hub.sock"), b"").unwrap();
    pid_of(&hub_host(&dir, "ensure").output().unwrap());
    stop(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_hub_executable_no_hub() {
    let dir = runtime_dir("none");
    let out = hub_host(&dir, "ensure")
        .env("WW_HUB_EXE", dir.join("no-such-hub"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("starting the ww hub"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hub_version_is_answered() {
    let out = Command::new(HUB_HOST)
        .arg("--hub-version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        ww_hub::HUB_VERSION
    );
}
