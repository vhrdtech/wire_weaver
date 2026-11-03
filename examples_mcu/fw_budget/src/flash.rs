//! Flash budget of a server (TEST-30): builds the `qemu-flash-*` bins of `mcu_qemu` (servers of the roots of
//! `tests/flash_budget_api`, see `mcu_qemu/src/flash_budget.rs`) and prints what the floor costs and what one more
//! resource of each kind costs, from the flash of the ELFs, plus checks for code and strings a firmware should not
//! carry without asking for it.

use crate::{Filter, Res, cargo, flash_size};
use object::{Object, ObjectSection, ObjectSymbol};
use std::path::{Path, PathBuf};

/// Resources of one kind the larger bin of a pair has more (`flash_budget_api::EXTRA`).
const EXTRA: u64 = 8;
/// Resources of `flash_budget_api::Base`: two of each kind.
const BASE: u64 = 8;
/// The profiles firmware ships with (`mcu_qemu/Cargo.toml`).
const PROFILES: [&str; 2] = ["fw-z", "fw-3"];

/// The pairs of bins, 8 and 16 resources of one kind at the root (`qemu-flash-<kind>8` / `16`), and the kind.
const KINDS: [(&str, &str); 7] = [
    ("qemu-flash-calls", "call, no arguments or output"),
    ("qemu-flash-calls-args", "call, an argument and an output"),
    ("qemu-flash-props", "property, read and write"),
    ("qemu-flash-streams", "stream"),
    (
        "qemu-flash-typed",
        "call, argument and output types of its own",
    ),
    (
        "qemu-flash-maxlen",
        "property of MaxLen<&str, N>, a different N each (SW-72)",
    ),
    (
        "qemu-flash-globals",
        "trait reached by a global path, introspection off (SRV-53)",
    ),
];

/// Something checked in a server bin: symbols whose demangled name contains one of `symbols`, and strings found
/// anywhere in the ELF's flash.
struct Check {
    what: &'static str,
    bin: &'static str,
    symbols: &'static [&'static str],
    strings: &'static [&'static str],
}

const CHECKS: [Check; 3] = [
    Check {
        what: "observe: the Series codec (SRV-54)",
        bin: "qemu-flash-streams16",
        symbols: &["shrink_wrap::series::"],
        strings: &[],
    },
    Check {
        what: "core::fmt Debug of shrink_wrap::Error (SW-69)",
        bin: "qemu-flash-streams16",
        symbols: &["<shrink_wrap::Error as core::fmt::Debug>::fmt"],
        strings: &["OutOfBoundsReadU8", "OutOfBoundsWriteU8"],
    },
    Check {
        what: "global trait paths by name (SRV-53), introspection off",
        bin: "qemu-flash-globals16",
        symbols: &["ww_global_trait_path"],
        strings: &["GlobalTrait15", "flash_budget_api"],
    },
];

pub fn run(targets: &[String], filter: &Filter, qemu_dir: &Path) -> Res<()> {
    for target in targets {
        for (profile, async_) in filter.pairs(&PROFILES) {
            print_one(qemu_dir, target, profile, async_)?;
        }
    }
    Ok(())
}

fn print_one(qemu_dir: &Path, target: &str, profile: &str, async_: bool) -> Res<()> {
    let mode = if async_ { "use_async" } else { "sync" };
    eprintln!("building the qemu-flash-* bins for {target} at {profile}, {mode}");
    let mut bins: Vec<String> = vec!["qemu-flash-floor".into(), "qemu-flash-base-only".into()];
    for (bin, _) in KINDS {
        bins.push(format!("{bin}8"));
        bins.push(format!("{bin}16"));
    }
    let mut cmd = cargo(qemu_dir);
    cmd.args(["build", "--profile", profile, "--target", target]);
    for bin in &bins {
        cmd.args(["--bin", bin]);
    }
    if async_ {
        cmd.args(["--features", "flash-async"]);
    }
    if !cmd.status()?.success() {
        return Err("cargo build of the qemu-flash-* bins failed".into());
    }
    let dir: PathBuf = qemu_dir.join("target").join(target).join(profile);
    let size = |bin: &str| flash_size(&dir.join(bin));
    let floor = size("qemu-flash-floor")?;
    let base = size("qemu-flash-base-only")?;
    println!(
        "\n{target}, {profile}, {mode}: flash of the whole image (code, read-only data, vector table)"
    );
    println!("  {:<46} {floor:>7} B", "floor, one method");
    println!(
        "  {:<46} {base:>7} B  ({:+} B, {} B per resource)",
        format!("{BASE} resources, two of each kind"),
        base as i64 - floor as i64,
        (base as i64 - floor as i64) / BASE as i64
    );
    let mut sum = 0.0;
    for (bin, kind) in KINDS {
        let (fewer, more) = (size(&format!("{bin}8"))?, size(&format!("{bin}16"))?);
        let per = (more as i64 - fewer as i64) as f64 / EXTRA as f64;
        if !matches!(
            bin,
            "qemu-flash-typed" | "qemu-flash-maxlen" | "qemu-flash-globals"
        ) {
            sum += per;
        }
        println!("  {:<46} {per:>7.0} B", format!("one more {kind}"));
    }
    println!(
        "  {:<46} {:>7.0} B",
        "mean of the four kinds of shared types",
        sum / 4.0
    );
    for check in &CHECKS {
        let elf = dir.join(check.bin);
        let (syms, bytes) = symbols(&elf, check.symbols)?;
        let found: Vec<_> = check
            .strings
            .iter()
            .filter(|s| contains(&elf, s).unwrap_or(false))
            .collect();
        if syms == 0 && found.is_empty() {
            println!("  {}: not linked", check.what);
        } else {
            let strings = if found.is_empty() {
                String::new()
            } else {
                format!(", strings {found:?}")
            };
            println!("  {}: {syms} symbol(s), {bytes} B{strings}", check.what);
        }
    }
    Ok(())
}

/// Count and bytes of the symbols of `elf` whose demangled name contains one of `names`.
fn symbols(elf: &Path, names: &[&str]) -> Res<(usize, u64)> {
    let data = std::fs::read(elf)?;
    let file = object::File::parse(&*data)?;
    let mut count = 0;
    let mut bytes = 0;
    for s in file.symbols() {
        let Ok(name) = s.name() else { continue };
        let name = format!("{:#}", rustc_demangle::demangle(name));
        if names.iter().any(|n| name.contains(n)) {
            count += 1;
            bytes += s.size();
        }
    }
    Ok((count, bytes))
}

/// Whether `text` is in a section of `elf` that goes to flash.
fn contains(elf: &Path, text: &str) -> Res<bool> {
    let data = std::fs::read(elf)?;
    let file = object::File::parse(&*data)?;
    for s in file.sections() {
        let alloc =
            matches!(s.flags(), object::SectionFlags::Elf { sh_flags } if sh_flags & 2 != 0);
        if !alloc || s.kind() == object::SectionKind::UninitializedData {
            continue;
        }
        if s.data()?.windows(text.len()).any(|w| w == text.as_bytes()) {
            return Ok(true);
        }
    }
    Ok(false)
}
