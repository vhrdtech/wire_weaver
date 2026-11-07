//! The no-float firmware guard (TEST-33, see our internal design notes, section 3 and
//! D2): builds `qemu-no-f64-guard` (`examples_mcu/mcu_qemu/src/bin/no_f64_guard.rs`), a fixture that calls every
//! device-relevant `ww_si::Quantity` operation, on the cores and number-feature combinations real
//! firmware uses, and lists what each ELF links that its group forbids, with bytes.
//!
//! Two groups, by what the firmware opted into (D2: no floats on a device unless a type is a float or a feature
//! turns them on):
//! - no floats (`u32`, `u32+u64`): any f32 or f64 helper fails, on `thumbv7em-none-eabihf` also any VFP
//!   arithmetic instruction in the disassembly (that core has an FPU and so no helpers);
//! - opted in (`u32+f32`, `u32+u64+f32`): f32 is allowed, f64 fails.
//!
//! Both fail on 128-bit helpers and on the named slow paths (`to_ratio`, `compare_slow`, ...), which catch math the
//! compiler inlined where no helper symbol is left. A path inlined into its caller leaves neither; that stays
//! unseen here. A forbidden symbol fails the run (`just no-float-guard`, part of `just test` since STD-102 moved
//! `ww_si` onto exact_units' integer kernel); `--guard-report` only reports.

use crate::{Res, cargo, matching_symbols};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

const BIN: &str = "qemu-no-f64-guard";
/// The cores this guard checks: a Cortex-M0(+) with no FPU and a Cortex-M4F with a single-precision one (f64 stays
/// soft-float on both).
const TARGETS: [&str; 2] = ["thumbv6m-none-eabi", "thumbv7em-none-eabihf"];
/// The profiles firmware ships with (`mcu_qemu/Cargo.toml`).
const PROFILES: [&str; 2] = ["fw-z", "fw-3"];

/// A number-feature set of the fixture, as firmware has it.
struct Config {
    label: &'static str,
    /// `mcu_qemu`'s default features carry `f32`: the no-float group builds without them.
    floats: bool,
    features: &'static [&'static str],
}

const CONFIGS: [Config; 4] = [
    Config {
        label: "u32",
        floats: false,
        features: &[],
    },
    Config {
        label: "u32+u64",
        floats: false,
        features: &["guard-u64"],
    },
    Config {
        label: "u32+f32",
        floats: true,
        features: &[],
    },
    Config {
        label: "u32+u64+f32",
        floats: true,
        features: &["guard-u64"],
    },
];

/// f32 soft-float: the ARM EABI helpers and compiler-rt's single-precision arithmetic, comparison and conversion
/// routines. `__aeabi_f2d` (f32 to f64) is in [F64_SYMBOLS].
const F32_SYMBOLS: &[&str] = &[
    "__aeabi_f",
    "__aeabi_i2f",
    "__aeabi_ui2f",
    "__aeabi_l2f",
    "__aeabi_ul2f",
    "addsf3",
    "subsf3",
    "mulsf3",
    "divsf3",
    "negsf2",
    "eqsf2",
    "nesf2",
    "ltsf2",
    "lesf2",
    "gtsf2",
    "gesf2",
    "unordsf2",
    "cmpsf2",
    "fixsfsi",
    "fixsfdi",
    "fixunssfsi",
    "fixunssfdi",
    "floatsisf",
    "floatdisf",
    "floatunsisf",
    "floatundisf",
];

/// f64 soft-float: the ARM EABI helpers (`__aeabi_d*`, plus the few whose name doesn't start with `d`) and the
/// compiler-rt double arithmetic, comparison and conversion routines they call.
const F64_SYMBOLS: &[&str] = &[
    "__aeabi_d",
    "__aeabi_f2d",
    "__aeabi_i2d",
    "__aeabi_ui2d",
    "__aeabi_l2d",
    "__aeabi_ul2d",
    "adddf3",
    "subdf3",
    "muldf3",
    "divdf3",
    "negdf2",
    "eqdf2",
    "nedf2",
    "ltdf2",
    "ledf2",
    "gtdf2",
    "gedf2",
    "unorddf2",
    "cmpdf2",
    "fixdfsi",
    "fixdfdi",
    "fixunsdfsi",
    "fixunsdfdi",
    "floatsidf",
    "floatdidf",
    "floatunsidf",
    "floatundidf",
    "extendsfdf2",
    "truncdfsf2",
];

/// 128-bit helpers: compiler-rt's `ti` (two-word integer) routines and the u128/i128 to float conversions.
const INT128_SYMBOLS: &[&str] = &[
    "multi3",
    "udivti3",
    "divti3",
    "umodti3",
    "modti3",
    "floattitf",
    "floatuntitf",
    "fixtfti",
    "fixunstfti",
    "floattidf",
    "floatuntidf",
    "floattisf",
    "floatuntisf",
];

/// The slow paths of `ww_si` and `exact_units` that do f64 or 128-bit math: found by their own names, so math the
/// compiler inlined (where no helper is called) still shows while the function keeps a symbol.
const SLOW_PATHS: &[&str] = &[
    "to_ratio",
    "compare_slow",
    "cmp_limit_slow",
    "rescale_slow",
    "apply_truncating",
    "apply_f64",
    "round_f64",
    "pow_half",
    "sqrt_f64",
    "int_sqrt",
];

/// Whether a symbol set breaks the rules of a group.
#[derive(Default)]
struct Row {
    f32_syms: Vec<(String, u64)>,
    f64_syms: Vec<(String, u64)>,
    int128_syms: Vec<(String, u64)>,
    slow_syms: Vec<(String, u64)>,
    /// VFP arithmetic instructions in the disassembly (`.f32` / `.f64` mnemonics), `thumbv7em-none-eabihf` only.
    vfp: usize,
}

impl Row {
    /// What the group forbids that this build links.
    fn violations(&self, floats: bool) -> usize {
        let mut n = self.f64_syms.len() + self.int128_syms.len() + self.slow_syms.len();
        if !floats {
            n += self.f32_syms.len() + self.vfp;
        }
        n
    }
}

pub fn run(qemu_dir: &Path, strict: bool) -> Res<()> {
    println!(
        "\nno-float guard (TEST-33): {BIN}; no floats = u32 and u32+u64 (f32, f64, 128-bit and VFP fail), opted in = with f32 (f64 and 128-bit fail)"
    );
    let mut failing = 0;
    for target in TARGETS {
        for profile in PROFILES {
            for config in &CONFIGS {
                let row = build_and_scan(qemu_dir, target, profile, config)?;
                let bad = row.violations(config.floats);
                failing += usize::from(bad > 0);
                print_row(target, profile, config, &row, bad);
            }
        }
    }
    if strict && failing > 0 {
        return Err(format!("{failing} build(s) link code their group forbids").into());
    }
    Ok(())
}

fn build_and_scan(qemu_dir: &Path, target: &str, profile: &str, config: &Config) -> Res<Row> {
    eprintln!("building {BIN} for {target} at {profile}, {}", config.label);
    let mut cmd = cargo(qemu_dir);
    cmd.args([
        "build",
        "--profile",
        profile,
        "--target",
        target,
        "--bin",
        BIN,
    ]);
    if !config.floats {
        cmd.arg("--no-default-features");
    }
    if !config.features.is_empty() {
        cmd.args(["--features", &config.features.join(",")]);
    }
    if !cmd.status()?.success() {
        return Err(format!("cargo build of {BIN} failed").into());
    }
    let elf = qemu_dir.join("target").join(target).join(profile).join(BIN);
    let mut row = Row {
        f32_syms: matching_symbols(&elf, F32_SYMBOLS)?,
        f64_syms: matching_symbols(&elf, F64_SYMBOLS)?,
        int128_syms: matching_symbols(&elf, INT128_SYMBOLS)?,
        slow_syms: matching_symbols(&elf, SLOW_PATHS)?,
        vfp: 0,
    };
    if target.ends_with("eabihf") {
        row.vfp = vfp_instructions(&elf)?;
    }
    Ok(row)
}

/// Instructions in the disassembly of `elf` that do VFP arithmetic or conversion, those with a `.f32` or `.f64`
/// in the mnemonic (`vadd.f32`, `vcvt.f32.s32`, `vsqrt.f64`); plain `vldr`, `vstr` and `vmov` only move words.
fn vfp_instructions(elf: &Path) -> Res<usize> {
    let out = Command::new("llvm-objdump")
        .args(["-d", "--no-show-raw-insn"])
        .arg(elf)
        .output()?;
    if !out.status.success() {
        return Err("llvm-objdump failed".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|l| l.split_once(':').map(|(_, rest)| rest.trim_start()))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|m| m.starts_with('v') && (m.contains(".f32") || m.contains(".f64")))
        .count())
}

fn bytes(syms: &[(String, u64)]) -> u64 {
    syms.iter().map(|(_, b)| b).sum()
}

fn print_row(target: &str, profile: &str, config: &Config, row: &Row, bad: usize) {
    let group = if config.floats {
        "opted in"
    } else {
        "no floats"
    };
    let verdict = if bad == 0 { "ok" } else { "FAIL" };
    println!(
        "  {target:<22} {profile:<5} {:<12} {group:<9} {verdict:<4} f32 {:>2} ({:>5} B)  f64 {:>2} ({:>5} B)  128-bit {:>2} ({:>4} B)  slow paths {:>2} ({:>4} B)  VFP insns {}",
        config.label,
        row.f32_syms.len(),
        bytes(&row.f32_syms),
        row.f64_syms.len(),
        bytes(&row.f64_syms),
        row.int128_syms.len(),
        bytes(&row.int128_syms),
        row.slow_syms.len(),
        bytes(&row.slow_syms),
        row.vfp
    );
    let mut per_crate: BTreeMap<&str, u64> = BTreeMap::new();
    let f32_forbidden = if config.floats {
        &[][..]
    } else {
        &row.f32_syms[..]
    };
    for (name, size) in f32_forbidden
        .iter()
        .chain(&row.f64_syms)
        .chain(&row.int128_syms)
        .chain(&row.slow_syms)
    {
        *per_crate.entry(crate_of(name)).or_default() += size;
    }
    if !per_crate.is_empty() {
        let list: Vec<String> = per_crate
            .iter()
            .map(|(c, b)| format!("{c} {b} B"))
            .collect();
        println!("      by crate: {}", list.join(", "));
    }
}

/// The crate of a demangled symbol (`exact_units::fixed::f` is `exact_units`; `<T as Trait>::f` the type's); a
/// plain C name (`__aeabi_dadd`, `__multi3`) is a compiler-rt helper, `compiler_builtins`.
fn crate_of(name: &str) -> &str {
    let name = name.trim_start_matches('<');
    match name.split_once("::") {
        Some((krate, _)) => krate,
        None => "compiler_builtins",
    }
}
