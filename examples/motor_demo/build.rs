//! `motor_demo --version`: version, git SHA (`-dirty` with uncommitted changes) and commit time, `unknown` without git.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    if let Some(git_dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        println!("cargo:rerun-if-changed={git_dir}/index");
    }
    let version = env!("CARGO_PKG_VERSION");
    let long = match (
        git(&["rev-parse", "--short", "HEAD"]),
        git(&["log", "-1", "--format=%cd", "--date=format:%-d %b %Y %H:%M"]),
    ) {
        (Some(sha), Some(time)) => {
            let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
            let dirty = if dirty { "-dirty" } else { "" };
            format!("{version} ({sha}{dirty}, committed {time})")
        }
        _ => format!("{version} (unknown)"),
    };
    println!("cargo:rustc-env=MOTOR_DEMO_VERSION={long}");
}
