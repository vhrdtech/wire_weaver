//! The `optimize-speed` feature puts `#[optimize(speed)]` on the hot bit paths. The attribute is stable from Rust
//! 1.101, needs `#![feature(optimize_attribute)]` on an older nightly, and is not there on an older stable compiler,
//! where the feature is ignored with a warning (so `--all-features` still builds).

use std::process::Command;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(shrink_wrap_optimize)");
    println!("cargo::rustc-check-cfg=cfg(shrink_wrap_optimize_gate)");
    if std::env::var_os("CARGO_FEATURE_OPTIMIZE_SPEED").is_none() {
        return;
    }
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let version = Command::new(rustc)
        .arg("--version")
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    let version = String::from_utf8_lossy(&version);
    // "rustc 1.101.0-nightly (..)"
    let minor = version
        .split(['.', ' '])
        .nth(2)
        .and_then(|m| m.parse::<u32>().ok())
        .unwrap_or(0);
    if minor >= 101 {
        println!("cargo::rustc-cfg=shrink_wrap_optimize");
    } else if version.contains("-nightly") || version.contains("-dev") {
        println!("cargo::rustc-cfg=shrink_wrap_optimize");
        println!("cargo::rustc-cfg=shrink_wrap_optimize_gate");
    } else {
        println!(
            "cargo::warning=shrink_wrap: `optimize-speed` needs Rust 1.101 or a nightly compiler, ignored on {}",
            version.trim()
        );
    }
}
