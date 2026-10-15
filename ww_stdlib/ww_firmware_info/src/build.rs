//! Build-script helper: gathers git, version and time facts for [firmware_summary!](crate::firmware_summary).
//!
//! Call [emit] from the firmware's `build.rs`. It runs `git` (like our CLIs' `--version` does) and sets the
//! `WW_FW_*` variables with `cargo:rustc-env`. `rerun-if-changed` covers `.git/HEAD` and `.git/index`, so the commit
//! and dirty flag stay fresh. Outside git the commit is empty and the summary has `git: None`.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The facts `emit` sets as `cargo:rustc-env` lines, as `(name, value)` pairs.
pub fn facts() -> Vec<(&'static str, String)> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64));
    vec![
        ("WW_FW_NAME", env("CARGO_PKG_NAME")),
        ("WW_FW_VERSION_MAJOR", env("CARGO_PKG_VERSION_MAJOR")),
        ("WW_FW_VERSION_MINOR", env("CARGO_PKG_VERSION_MINOR")),
        ("WW_FW_VERSION_PATCH", env("CARGO_PKG_VERSION_PATCH")),
        ("WW_FW_VERSION_PRE", env("CARGO_PKG_VERSION_PRE")),
        ("WW_FW_GIT_COMMIT", commit),
        ("WW_FW_GIT_DIRTY", if dirty { "1".into() } else { String::new() }),
        ("WW_FW_BUILT", format_utc(secs)),
        ("WW_FW_PROFILE", env("PROFILE")),
    ]
}

/// Print the build facts for cargo, call it from `build.rs`.
pub fn emit() {
    for (k, v) in facts() {
        println!("cargo:rustc-env={k}={v}");
    }
    if let Some(git_dir) = Command::new("git").args(["rev-parse", "--git-dir"]).output().ok().filter(|o| o.status.success()) {
        let dir = String::from_utf8_lossy(&git_dir.stdout).trim().to_string();
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/index");
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
}

/// `YYYY-MM-DDTHH:MM:SS` of Unix seconds, UTC.
pub fn format_utc(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::format_utc;

    #[test]
    fn dates() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00");
        assert_eq!(format_utc(1_760_552_100), "2025-10-15T18:15:00");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00");
    }
}
