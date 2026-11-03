//! The no-f64 firmware guard (TEST-33, `~/git/tpm/prj/P2401-wireweaver/design/ww-si-exact-units.md` section 3):
//! builds `qemu-no-f64-guard` (`examples_mcu/mcu_qemu/src/bin/no_f64_guard.rs`), a fixture that calls every
//! device-relevant `ww_si::Quantity` operation of today's API, on the cores and number-feature combinations real
//! firmware uses, and lists the f64 soft-float and 128-bit helper symbols linked into each ELF, with their bytes.
//! Reports only for now (expected red everywhere): the gate (a build failing when one of these symbols appears)
//! turns on once STD-102 moves `ww_si` onto exact_units' integer kernel.

use crate::{Res, cargo, symbols};
use std::path::Path;

const BIN: &str = "qemu-no-f64-guard";
/// The cores this guard checks: a Cortex-M0(+) with no FPU and a Cortex-M4F with a single-precision one (f64 stays
/// soft-float on both).
const TARGETS: [&str; 2] = ["thumbv6m-none-eabi", "thumbv7em-none-eabihf"];
/// The profiles firmware ships with (`mcu_qemu/Cargo.toml`).
const PROFILES: [&str; 2] = ["fw-z", "fw-3"];
/// `(label, extra cargo features)`: today's wire_weaver/typec_dongle number types, and with a 64-bit type enabled
/// too, as usb_io_fw's firmware has (`ww_numeric::Integer` then 64-bit wide).
const FEATURE_SETS: [(&str, &[&str]); 2] = [("u32+f32", &[]), ("u32+u64+f32", &["guard-u64"])];

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

pub fn run(qemu_dir: &Path) -> Res<()> {
    println!(
        "\nno-f64 guard (TEST-33): {BIN}, f64 soft-float and 128-bit helper symbols linked into the ELF"
    );
    for target in TARGETS {
        for profile in PROFILES {
            for (label, features) in FEATURE_SETS {
                print_one(qemu_dir, target, profile, label, features)?;
            }
        }
    }
    Ok(())
}

fn print_one(
    qemu_dir: &Path,
    target: &str,
    profile: &str,
    label: &str,
    features: &[&str],
) -> Res<()> {
    eprintln!("building {BIN} for {target} at {profile}, {label}");
    let mut cmd = cargo(qemu_dir);
    cmd.args(["build", "--profile", profile, "--target", target, "--bin", BIN]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    if !cmd.status()?.success() {
        return Err(format!("cargo build of {BIN} failed").into());
    }
    let elf = qemu_dir.join("target").join(target).join(profile).join(BIN);
    let (f64_count, f64_bytes) = symbols(&elf, F64_SYMBOLS)?;
    let (int128_count, int128_bytes) = symbols(&elf, INT128_SYMBOLS)?;
    println!(
        "  {target:<22} {profile:<5} {label:<12} f64 {f64_count:>2} symbol(s) {f64_bytes:>5} B   128-bit {int128_count:>2} symbol(s) {int128_bytes:>5} B"
    );
    Ok(())
}
