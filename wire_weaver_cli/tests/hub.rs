//! `ww hub` against a real hub process in a private runtime dir (TOOL-17).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn ww(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ww"))
        .args(args)
        .env("WW_RUNTIME_DIR", dir)
        .env_remove("NO_COLOR")
        .output()
        .expect("run ww")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn serve(dir: &Path) -> Child {
    let child = Command::new(env!("CARGO_BIN_EXE_ww"))
        .args(["hub", "serve", "--idle-exit", "60"])
        .env("WW_RUNTIME_DIR", dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn hub");
    let start = Instant::now();
    while ww(dir, &["hub", "status"]).status.code() != Some(0) {
        assert!(start.elapsed() < Duration::from_secs(10), "hub came up");
        std::thread::sleep(Duration::from_millis(50));
    }
    child
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww-cli-hub-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn without_a_hub() {
    let dir = temp_dir("none");
    let status = ww(&dir, &["hub", "status"]);
    assert_eq!(status.status.code(), Some(1));
    assert!(stdout(&status).contains("no hub running"));
    // stop is idempotent, keep needs a hub to keep
    assert!(ww(&dir, &["hub", "stop"]).status.success());
    let keep = ww(&dir, &["hub", "keep", "1h"]);
    assert!(!keep.status.success());
    assert!(String::from_utf8_lossy(&keep.stderr).contains("no hub is running"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_keep_who_stop() {
    let dir = temp_dir("run");
    let mut hub = serve(&dir);

    let text = stdout(&ww(&dir, &["hub", "status"]));
    assert!(text.contains("running"), "{text}");
    assert!(text.contains(&format!("pid        {}", hub.id())), "{text}");
    assert!(text.contains("started by"), "{text}");
    assert!(text.contains("after 1m 00s without connections"), "{text}");
    // piped: no colour
    assert!(!text.contains('\x1b'), "{text:?}");

    let keep = ww(&dir, &["hub", "keep", "2h"]);
    assert!(keep.status.success());
    assert!(stdout(&keep).contains("2h 00m"), "{}", stdout(&keep));
    let kept = stdout(&ww(&dir, &["hub", "status"]));
    assert!(
        kept.contains("kept up for") && kept.contains("more"),
        "{kept}"
    );
    assert!(!ww(&dir, &["hub", "keep", "5x"]).status.success());
    assert!(ww(&dir, &["hub", "keep", "forever"]).status.success());
    assert!(stdout(&ww(&dir, &["hub", "status"])).contains("kept up until stopped"));

    // the asking program is listed as connected to the hub
    let who = ww(&dir, &["hub", "who"]);
    assert!(who.status.success());
    assert!(
        stdout(&who).contains("connected to the hub"),
        "{}",
        stdout(&who)
    );

    let stop = ww(&dir, &["hub", "stop"]);
    assert!(stop.status.success());
    assert!(stdout(&stop).contains("hub stopped"));
    hub.wait().unwrap();
    assert_eq!(ww(&dir, &["hub", "status"]).status.code(), Some(1));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn list_goes_through_the_hub() {
    let dir = temp_dir("list");
    let mut hub = serve(&dir);
    let list = ww(&dir, &["list"]);
    assert!(list.status.success());
    // with a hub the table has the hub's STATE column, `--direct` keeps the old one
    let text = stdout(&list);
    if !text.is_empty() {
        assert!(text.contains("STATE"), "{text}");
    }
    assert!(!stdout(&ww(&dir, &["list", "--direct"])).contains("STATE"));
    ww(&dir, &["hub", "stop"]);
    hub.wait().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn completions_come_with_help_text() {
    let dir = temp_dir("complete");
    let complete = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ww"))
            .env("COMPLETE", "fish")
            .env("WW_RUNTIME_DIR", &dir)
            .arg("--")
            .arg("ww")
            .args(args)
            .output()
            .unwrap();
        stdout(&out)
    };
    let keep = complete(&["hub", "keep", ""]);
    assert!(keep.contains("forever\tuntil `ww hub stop`"), "{keep}");
    let subs = complete(&["hub", ""]);
    for sub in ["serve", "status", "stop", "who", "keep"] {
        assert!(subs.contains(&format!("{sub}\t")), "{sub}: {subs}");
    }
    // device keys: whatever is plugged in, each with its help text
    for line in complete(&["hub", "who", ""])
        .lines()
        .filter(|l| l.starts_with("usb:"))
    {
        assert!(line.contains('\t'), "{line}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
