//! Runs the units firmware budget (TEST-27) and prints one table: `ww_si::Fixed` against the plain fields it
//! replaces, each case in instructions (Cortex-M3 under QEMU, `qemu_icount`), flash and stack per function (from the
//! ELF's symbols and `.stack_sizes`), and, with `--cycles`, in core clock cycles on the STM32H725 (DWT `CYCCNT`).
//!
//! ```text
//! cargo run --release -- --elf [--targets T1,T2]     # plain against Fixed code per function, see elf_diff.rs
//! cargo run --release -- [--profile fw-z|fw-3] [--qemu-host HOST] [--cycles [--chip CHIP]] [--bin-dir DIR]
//! ```
//!
//! The bench is `examples_mcu/mcu_qemu/src/unit_budget.rs`: every case is `<group>: plain` and `<group>: Fixed`
//! (the first variant of a group is its baseline), every function under test `plain_<name>` / `fixed_<name>`.
//! `--profile` is a profile of `mcu_qemu/Cargo.toml` (`fw-z` is the shipped firmware's: fat LTO, one codegen unit,
//! `opt-level = "z"`; `fw-3` the same at 3). `--qemu-host HOST` runs QEMU there over ssh (this PC has no
//! `qemu-system-arm`): the ELF and `qemu_icount` are copied to `/tmp/ww-units-budget` on HOST, which needs cargo.
//! `--cycles` flashes the board behind the probe (`PROBE_RS_PROBE=VID:PID` picks one), replacing its firmware.

mod elf_diff;

use object::{Object, ObjectSection, ObjectSymbol};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

const QEMU_BIN: &str = "qemu-unit-budget";
const CYCLES_BIN: &str = "stm32h725ig-unit-budget-cycles";
/// The Cortex-M cores of the ELF proof (`--elf`): M0/M0+, M4F/M7 and M33.
const ELF_TARGETS: [&str; 3] = [
    "thumbv6m-none-eabi",
    "thumbv7em-none-eabihf",
    "thumbv8m.main-none-eabihf",
];
/// The shipping opt-levels, as profiles of `mcu_qemu/Cargo.toml`.
const ELF_PROFILES: [&str; 2] = ["fw-z", "fw-3"];
/// Wall-clock seconds QEMU and the board run get before they are stopped.
const TIMEOUT_S: u32 = 300;

struct Args {
    profile: String,
    qemu_host: Option<String>,
    cycles: bool,
    chip: String,
    /// Compare the plain and Fixed code in the ELFs of these targets and both profiles instead of running anything.
    elf: bool,
    targets: Vec<String>,
}

fn parse_args() -> Res<Args> {
    let mut args = Args {
        profile: "fw-z".into(),
        qemu_host: None,
        cycles: false,
        chip: "STM32H725IGKx".into(),
        elf: false,
        targets: ELF_TARGETS.iter().map(|t| t.to_string()).collect(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--profile" => args.profile = value("--profile")?,
            "--qemu-host" => args.qemu_host = Some(value("--qemu-host")?),
            "--chip" => args.chip = value("--chip")?,
            "--cycles" => args.cycles = true,
            "--elf" => args.elf = true,
            "--targets" => {
                args.targets = value("--targets")?.split(',').map(str::to_string).collect()
            }
            _ => return Err(format!("unknown argument {a}, see the top of src/main.rs").into()),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    match parse_args().and_then(|a| run(&a)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `cargo` in `dir` with that project's own `rust-toolchain.toml` (the one this tool runs with is not passed down).
fn cargo(dir: &Path) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(dir).env_remove("RUSTUP_TOOLCHAIN");
    cmd
}

/// Runs `cmd`, its stderr passed through, and returns its stdout.
fn output(cmd: &mut Command) -> Res<String> {
    let out = cmd.stderr(Stdio::inherit()).output()?;
    if !out.status.success() {
        return Err(format!("{cmd:?} failed: {}", out.status).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Symbol size of `name` (demangled) in `elf`; `None` when the linker left it out.
pub fn flash_of_symbol(elf: &Path, name: &str) -> Option<u64> {
    let data = std::fs::read(elf).ok()?;
    let file = object::File::parse(&*data).ok()?;
    file.symbols()
        .find(|s| {
            s.name()
                .is_ok_and(|n| format!("{:#}", rustc_demangle::demangle(n)) == name)
        })
        .map(|s| s.size())
}

/// Builds the bench for every core and profile and compares the plain and Fixed code of each pair (a differing pair is the
/// one worth measuring on a board).
fn run_elf(args: &Args, qemu_dir: &Path) -> Res<()> {
    let mut differing = 0;
    for target in &args.targets {
        for profile in ELF_PROFILES {
            eprintln!("building {QEMU_BIN} for {target} at profile {profile}");
            build_qemu_bin(qemu_dir, profile, "", target)?;
            let elf = qemu_dir
                .join("target")
                .join(target)
                .join(profile)
                .join(QEMU_BIN);
            let rows = elf_diff::print(&format!("{target}, profile {profile}"), &elf)?;
            differing += rows.iter().filter(|(_, v)| !v.identical()).count();
        }
    }
    println!(
        "\n{differing} pair(s) with different code: cycles on silicon are needed only for those; identical instructions cost identical cycles"
    );
    Ok(())
}

fn run(args: &Args) -> Res<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let qemu_dir = root.join("mcu_qemu");
    if args.elf {
        return run_elf(args, &qemu_dir);
    }
    eprintln!("building {QEMU_BIN} at profile {}", args.profile);
    let elf = qemu_dir
        .join("target/thumbv7m-none-eabi")
        .join(&args.profile)
        .join(QEMU_BIN);
    // the flash of a build with only the plain cases and one with only the Fixed ones, the whole image each
    let mut flash = Vec::new();
    for features in ["only-plain", "only-fixed", ""] {
        build_qemu_bin(&qemu_dir, &args.profile, features, "thumbv7m-none-eabi")?;
        flash.push(flash_size(&elf)?);
    }
    let funcs = functions(&elf)?;

    eprintln!("running {QEMU_BIN} under QEMU");
    let icount = qemu_icount(&root, &elf, args.qemu_host.as_deref())?;
    let instr = parse_icount(&icount)?;

    let cycles = if args.cycles {
        Some(board_cycles(&root, &args.chip)?)
    } else {
        None
    };

    println!("profile {} (Cortex-M3 under QEMU)", args.profile);
    print_cases(&instr, cycles.as_deref());
    println!(
        "\nflash of the whole image (code, read-only data, vector table): {} bytes with the plain cases only, {} with the Fixed cases only ({:+})",
        flash[0],
        flash[1],
        flash[1] as i64 - flash[0] as i64
    );
    print_functions(&funcs);
    Ok(())
}

/// Builds the QEMU bin with `features` and `-Z emit-stack-sizes`.
fn build_qemu_bin(dir: &Path, profile: &str, features: &str, target: &str) -> Res<()> {
    let mut cmd = cargo(dir);
    cmd.args([
        "rustc",
        "--profile",
        profile,
        "--target",
        target,
        "--bin",
        QEMU_BIN,
    ]);
    if !features.is_empty() {
        cmd.args(["--features", features]);
    }
    if !cmd
        .args(["--", "-Z", "emit-stack-sizes"])
        .status()?
        .success()
    {
        return Err("cargo rustc failed".into());
    }
    Ok(())
}

/// Bytes of the ELF that sit in flash: every allocated section that has contents (not `.bss`).
fn flash_size(elf: &Path) -> Res<u64> {
    let data = std::fs::read(elf)?;
    let file = object::File::parse(&*data)?;
    Ok(file
        .sections()
        .filter(|s| {
            let alloc =
                matches!(s.flags(), object::SectionFlags::Elf { sh_flags } if sh_flags & 2 != 0);
            alloc && s.kind() != object::SectionKind::UninitializedData
        })
        .map(|s| s.size())
        .sum())
}

/// What a case cost: cold run and warm run (the smallest of the others), for `ops` operations.
#[derive(Clone, Debug, PartialEq)]
struct Cost {
    label: String,
    ops: u64,
    cold: u64,
    warm: u64,
}

/// Instructions from `qemu_icount`'s output: the `bracket=N <label> run=R ops=K` lines the bin prints, then the
/// `bracket N: X instructions` lines the tool prints (N counts from 1).
fn parse_icount(text: &str) -> Res<Vec<Cost>> {
    let mut brackets: Vec<(String, u64, u64)> = Vec::new(); // label, run, ops
    let mut counts: BTreeMap<usize, u64> = BTreeMap::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("bracket=") {
            let (_, rest) = rest.split_once(' ').ok_or("bad bracket line")?;
            let (label, rest) = rest.rsplit_once(" run=").ok_or("bad bracket line")?;
            let (run, ops) = rest.split_once(" ops=").ok_or("bad bracket line")?;
            brackets.push((label.into(), run.parse()?, ops.parse()?));
        } else if let Some(rest) = line.strip_prefix("bracket ") {
            let (n, rest) = rest.split_once(": ").ok_or("bad count line")?;
            let count = rest.strip_suffix(" instructions").ok_or("bad count line")?;
            counts.insert(n.parse()?, count.parse()?);
        }
    }
    if brackets.is_empty() || counts.len() != brackets.len() {
        return Err(format!(
            "{} bracket lines but {} counts, did QEMU run to the end?\n{text}",
            brackets.len(),
            counts.len()
        )
        .into());
    }
    let samples = brackets
        .into_iter()
        .enumerate()
        .map(|(i, (label, run, ops))| (label, ops, run, counts[&(i + 1)]));
    Ok(collect(samples))
}

/// Groups `(label, ops, run, value)` samples into one [Cost] per label, in first-seen order.
fn collect(samples: impl Iterator<Item = (String, u64, u64, u64)>) -> Vec<Cost> {
    let mut costs: Vec<Cost> = Vec::new();
    for (label, ops, run, value) in samples {
        let i = match costs.iter().position(|c| c.label == label) {
            Some(i) => i,
            None => {
                costs.push(Cost {
                    label,
                    ops,
                    cold: 0,
                    warm: u64::MAX,
                });
                costs.len() - 1
            }
        };
        if run == 0 {
            costs[i].cold = value;
        } else {
            costs[i].warm = costs[i].warm.min(value);
        }
    }
    costs
}

/// Runs `qemu_icount` on the ELF, here or on `host` over ssh.
fn qemu_icount(root: &Path, elf: &Path, host: Option<&str>) -> Res<String> {
    let args = ["cortex-m3", "lm3s6965evb"];
    let limits = [
        "--max-insns=50000000".to_string(),
        format!("--timeout={TIMEOUT_S}"),
    ];
    let Some(host) = host else {
        return output(
            cargo(&root.join("qemu_icount"))
                .args(["run", "-q", "--release", "--"])
                .args(args)
                .arg(elf)
                .args(&limits),
        );
    };
    let remote = "/tmp/ww-units-budget";
    output(Command::new("ssh").args([host, &format!("mkdir -p {remote}")]))?;
    output(
        Command::new("rsync")
            .args(["-az", "--exclude", "target"])
            .arg(root.join("qemu_icount"))
            .arg(elf)
            .arg(format!("{host}:{remote}/")),
    )?;
    let elf_name = elf
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("bad elf path")?;
    output(Command::new("ssh").arg(host).arg(format!(
        "cd {remote}/qemu_icount && cargo run -q --release -- {} ../{elf_name} {}",
        args.join(" "),
        limits.join(" ")
    )))
}

/// Cycles from the board: builds the cycles bin and runs it with probe-rs until it prints `done`.
fn board_cycles(root: &Path, chip: &str) -> Res<Vec<Cost>> {
    let dir = root.join("usb_stm32h725ig");
    eprintln!("building {CYCLES_BIN}");
    let status = cargo(&dir)
        .args(["build", "--release", "--bin", CYCLES_BIN])
        .status()?;
    if !status.success() {
        return Err("cargo build of the cycles bin failed".into());
    }
    let elf = dir
        .join("target/thumbv7em-none-eabihf/release")
        .join(CYCLES_BIN);
    eprintln!("running it on {chip}");
    let text = output(
        Command::new("timeout")
            .args([&format!("{TIMEOUT_S}s"), "probe-rs", "run", "--chip", chip])
            .arg(elf),
    );
    // `timeout` ends probe-rs after the bin's `done` (it loops in wfi), so the exit status is no error here
    let text = match text {
        Ok(t) => t,
        Err(e) if e.to_string().contains("exit status: 124") => {
            return Err(format!(
                "no result from the board in {TIMEOUT_S} s (is {chip} behind the probe?)"
            )
            .into());
        }
        Err(e) => return Err(e),
    };
    parse_cycles(&text)
}

/// The defmt lines `<label> | ops=K cycles cold=C warm=W ..` of the cycles bin.
fn parse_cycles(text: &str) -> Res<Vec<Cost>> {
    let mut costs = Vec::new();
    for line in text.lines() {
        let Some((head, rest)) = line.split_once(" | ops=") else {
            continue;
        };
        let label = head.rsplit_once("INFO ").map_or(head, |(_, l)| l).trim();
        let field = |name: &str| -> Res<u64> {
            let (_, after) = rest.split_once(name).ok_or("bad cycles line")?;
            let num: String = after.chars().take_while(char::is_ascii_digit).collect();
            Ok(num.parse()?)
        };
        costs.push(Cost {
            label: label.into(),
            ops: rest.split(' ').next().ok_or("bad cycles line")?.parse()?,
            cold: field("cold=")?,
            warm: field("warm=")?,
        });
    }
    if costs.is_empty() {
        return Err(format!("no cycles lines in the output:\n{text}").into());
    }
    Ok(costs)
}

/// Splits `"group: variant"`.
fn split_label(label: &str) -> (&str, &str) {
    label.split_once(": ").unwrap_or((label, ""))
}

fn print_cases(instr: &[Cost], cycles: Option<&[Cost]>) {
    println!("\ninstructions per operation, warm run (the cold run in brackets)");
    print_table(instr);
    if let Some(cycles) = cycles {
        println!(
            "\nDWT cycles per operation on the STM32H725 (540 MHz), warm run (cold in brackets)"
        );
        print_table(cycles);
    }
}

fn print_table(costs: &[Cost]) {
    println!(
        "{:<20} {:>22} {:>22} {:>9}",
        "case", "plain", "Fixed", "Fixed/plain"
    );
    let mut groups: Vec<&str> = Vec::new();
    for c in costs {
        let g = split_label(&c.label).0;
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    for g in groups {
        let variants: Vec<&Cost> = costs
            .iter()
            .filter(|c| split_label(&c.label).0 == g)
            .collect();
        let cell = |c: &Cost| {
            format!(
                "{:.1} ({:.1})",
                c.warm as f64 / c.ops as f64,
                c.cold as f64 / c.ops as f64
            )
        };
        let ratio = match variants[..] {
            [a, b] => format!("{:.2}x", b.warm as f64 / a.warm as f64),
            _ => String::new(),
        };
        let cells: Vec<String> = variants.iter().map(|c| cell(c)).collect();
        println!(
            "{:<20} {:>22} {:>22} {:>9}",
            g,
            cells.first().map_or("", |s| s),
            cells.get(1).map_or("", |s| s),
            ratio
        );
    }
}

/// Flash and stack frame of one function.
#[derive(Default, Clone, Copy)]
struct Func {
    flash: u64,
    stack: Option<u64>,
}

/// `unit_budget::plain_*` / `fixed_*` of the ELF: name (without the prefix) to `(plain, fixed)`; a function the
/// linker folded into its twin (identical code) has no symbol and shows as folded.
fn functions(elf: &Path) -> Res<BTreeMap<String, [Option<Func>; 2]>> {
    let data = std::fs::read(elf)?;
    let file = object::File::parse(&*data)?;
    let mut stack: BTreeMap<u64, u64> = BTreeMap::new();
    if let Some(section) = file.section_by_name(".stack_sizes") {
        let bytes = section.data()?;
        let mut at = 0;
        while at + 4 <= bytes.len() {
            let addr = u32::from_le_bytes(bytes[at..at + 4].try_into()?) as u64 & !1;
            at += 4;
            let (size, n) = uleb128(&bytes[at..]).ok_or("bad .stack_sizes")?;
            at += n;
            stack.insert(addr, size);
        }
    }
    let mut out: BTreeMap<String, [Option<Func>; 2]> = BTreeMap::new();
    for sym in file.symbols() {
        let Ok(name) = sym.name() else { continue };
        let name = format!("{:#}", rustc_demangle::demangle(name));
        let Some((_, tail)) = name.rsplit_once("unit_budget::") else {
            continue;
        };
        let (side, key) = if let Some(k) = tail.strip_prefix("plain_") {
            (0, k)
        } else if let Some(k) = tail.strip_prefix("fixed_") {
            (1, k)
        } else {
            continue;
        };
        out.entry(key.into()).or_default()[side] = Some(Func {
            flash: sym.size(),
            stack: stack.get(&(sym.address() & !1)).copied(),
        });
    }
    Ok(out)
}

/// The value and the length of the LEB128 number at the start of `bytes`.
fn uleb128(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, b) in bytes.iter().enumerate() {
        value |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

fn print_functions(funcs: &BTreeMap<String, [Option<Func>; 2]>) {
    println!(
        "\nown code and stack frame per function of the full build (folded: the linker merged it with its twin; a callee outside the function is not counted)"
    );
    println!(
        "{:<20} {:>14} {:>14} {:>14} {:>14}",
        "function", "flash plain", "flash Fixed", "stack plain", "stack Fixed"
    );
    let cell = |f: Option<Func>, stack: bool| match f {
        None => "folded".to_string(),
        Some(f) if stack => f.stack.map_or("?".into(), |s| s.to_string()),
        Some(f) => f.flash.to_string(),
    };
    for (name, [plain, fixed]) in funcs {
        println!(
            "{:<20} {:>14} {:>14} {:>14} {:>14}",
            name,
            cell(*plain, false),
            cell(*fixed, false),
            cell(*plain, true),
            cell(*fixed, true)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icount_pairs_brackets_with_counts() {
        let text = "bracket=0 struct encode: plain run=0 ops=16\n\
                    bracket=1 struct encode: plain run=1 ops=16\n\
                    bracket=2 struct encode: Fixed run=0 ops=16\n\
                    bracket=3 struct encode: Fixed run=1 ops=16\n\
                    struct encode: plain | bytes=1 hash=2\n\
                    bracket 1: 100 instructions\nbracket 2: 90 instructions\n\
                    bracket 3: 120 instructions\nbracket 4: 95 instructions\n";
        let costs = parse_icount(text).unwrap();
        assert_eq!(costs.len(), 2);
        assert_eq!((costs[0].cold, costs[0].warm, costs[0].ops), (100, 90, 16));
        assert_eq!((costs[1].cold, costs[1].warm), (120, 95));
    }

    #[test]
    fn cycles_line_of_the_board() {
        let text = "0.001 INFO struct encode: Fixed | ops=16 cycles cold=900 warm=700 per op x10=437 | bytes=224 hash=5afa8c10\n";
        let costs = parse_cycles(text).unwrap();
        assert_eq!(costs[0].label, "struct encode: Fixed");
        assert_eq!((costs[0].ops, costs[0].cold, costs[0].warm), (16, 900, 700));
    }

    #[test]
    fn leb128() {
        assert_eq!(uleb128(&[0x05]), Some((5, 1)));
        assert_eq!(uleb128(&[0xe5, 0x8e, 0x26]), Some((624485, 3)));
    }
}
