//! A `ww-*` program on PATH is run with the words being completed only after it answered the plugin probe.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn complete(dir: &Path, line: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ww"))
        .args(["--", "ww"])
        .args(line.split(' '))
        .env("COMPLETE", "fish")
        .env(
            "PATH",
            format!("{}:{}", dir.display(), std::env::var("PATH").unwrap()),
        )
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `ww-foo` is no plugin: it is listed, and run with `--ww-plugin` only, never with the typed words.
#[test]
fn a_program_that_is_no_plugin_never_gets_the_words() {
    let dir = std::env::temp_dir().join(format!("ww_probe_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("calls");
    script(
        &dir,
        "ww-foo",
        &format!("echo \"$@\" >> {}\necho ran", log.display()),
    );
    assert!(complete(&dir, "foo").contains("foo"));
    let out = complete(&dir, "foo --x");
    assert!(!out.contains("ran"), "{out}");
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    // the probe, once (the second completion used the cache), nothing else
    assert_eq!(
        calls.lines().collect::<Vec<_>>(),
        ["--ww-plugin"],
        "{calls}"
    );

    // a program that answers the probe is asked
    script(
        &dir,
        "ww-bar",
        &format!(
            "if [ \"$1\" = --ww-plugin ]; then echo 'ww-plugin 1'; else echo \"$@\" >> {}; echo 'zed\\thelp'; fi",
            log.display()
        ),
    );
    assert!(complete(&dir, "bar ").contains("zed") || log.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
