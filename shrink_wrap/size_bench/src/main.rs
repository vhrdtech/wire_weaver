//! Builds the firmware images in `fw/` (one test case each) for small CPU targets and prints their code size,
//! or the largest symbols of one image. See `docs/serdes/code_size.md` for what the numbers mean.

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Test cases of `fw/` (its cargo features); `true`: the case uses derived types, so it has a `final` flavor.
const CASES: &[(&str, bool)] = &[
    ("empty", false),
    ("rw_de", false),
    ("rw_ser", false),
    ("struct_de", true),
    ("struct_ser", true),
    ("enum_de", true),
    ("enum_ser", true),
    ("vec_de", true),
    ("vec_ser", true),
    ("option_de", true),
    ("option_ser", true),
    ("busgen_de", true),
    ("busgen_ser", true),
    ("busgen", true),
    ("busgen_hand", false),
];

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Target {
    /// RISC-V RV32I, no compressed instructions
    Rv32i,
    /// RISC-V RV32IC (the QERV softcore of fpga_tools' busgen)
    Rv32ic,
    /// Cortex-M0
    Thumbv6m,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Target::Rv32i => "rv32i",
            Target::Rv32ic => "rv32ic",
            Target::Thumbv6m => "thumbv6m",
        }
    }

    fn triple(self) -> &'static str {
        match self {
            Target::Rv32i | Target::Rv32ic => "riscv32i-unknown-none-elf",
            Target::Thumbv6m => "thumbv6m-none-eabi",
        }
    }

    fn rustflags(self) -> String {
        let common = "-C link-arg=-Tlink.x -Zlocation-detail=none -Zfmt-debug=none";
        match self {
            Target::Rv32ic => format!("{common} -C target-feature=+c"),
            _ => common.to_string(),
        }
    }
}

#[derive(Parser)]
#[command(about, version)]
struct Args {
    /// Targets to build for, default: all
    #[arg(short, long, value_enum, value_delimiter = ',')]
    target: Vec<Target>,
    /// Cases to build, default: all
    #[arg(short, long, value_delimiter = ',')]
    case: Vec<String>,
    /// Only the flavor with size assumptions (`sized` / `final_structure` types), or only the evolvable one
    #[arg(long)]
    flavor: Option<Flavor>,
    /// shrink_wrap features to build with, e.g. `tiny`
    #[arg(long, value_delimiter = ',')]
    sw_features: Vec<String>,
    /// Instead of the table: the N largest symbols of each image
    #[arg(short, long, value_name = "N")]
    symbols: Option<usize>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Flavor {
    /// Types as derived by default: `Unsized`, fields can be added later
    Evolvable,
    /// `sized` where the fields allow it, else `final_structure`
    Final,
}

struct Image {
    text: u64,
    rodata: u64,
    /// (size, name), largest first
    symbols: Vec<(u64, String)>,
}

fn build(fw: &Path, target: Target, case: &str, flavor: Flavor, sw_features: &[String]) -> Result<Image> {
    let mut features = vec![case.to_string()];
    if flavor == Flavor::Final {
        features.push("final".into());
    }
    features.extend(sw_features.iter().map(|f| format!("shrink_wrap/{f}")));
    // one target dir per target: RV32I and RV32IC share a triple and differ in RUSTFLAGS only
    let target_dir = fw.join("target").join(target.name());
    let mut cmd = Command::new("cargo");
    cmd.current_dir(fw)
        .args(["build", "--release", "--quiet", "--target", target.triple(), "--features", &features.join(",")])
        .arg("--target-dir")
        .arg(&target_dir)
        .env("RUSTFLAGS", target.rustflags());
    // fw/ has its own toolchain file; don't hand down the toolchain this runner was started with
    for var in ["RUSTUP_TOOLCHAIN", "RUSTC", "RUSTDOC", "CARGO", "CARGO_TARGET_DIR", "CARGO_ENCODED_RUSTFLAGS"] {
        cmd.env_remove(var);
    }
    let status = cmd.status().context("running cargo")?;
    if !status.success() {
        bail!("build of case {case} for {} failed", target.name());
    }
    let elf = target_dir.join(target.triple()).join("release/size_bench_fw");
    let data = std::fs::read(&elf).with_context(|| format!("reading {}", elf.display()))?;
    let file = object::File::parse(&*data)?;
    let size = |name| file.section_by_name(name).map_or(0, |s| s.size());
    let mut symbols: Vec<(u64, String)> = file
        .symbols()
        .filter(|s| s.size() > 0 && matches!(s.kind(), SymbolKind::Text | SymbolKind::Data))
        .filter(|s| {
            let section = s.section_index().and_then(|i| file.section_by_index(i).ok());
            section.is_some_and(|sec| matches!(sec.name(), Ok(".text" | ".rodata")))
        })
        .map(|s| (s.size(), format!("{:#}", rustc_demangle::demangle(s.name().unwrap_or("?")))))
        .collect();
    symbols.sort_by(|a, b| b.cmp(a));
    Ok(Image { text: size(".text"), rodata: size(".rodata"), symbols })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let fw = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fw");
    let targets = if args.target.is_empty() { Target::value_variants().to_vec() } else { args.target };
    for case in &args.case {
        if !CASES.iter().any(|(name, _)| name == case) {
            bail!("no case `{case}`, there are: {}", CASES.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", "));
        }
    }
    let mut rows = vec![];
    for &(case, has_final) in CASES {
        if !args.case.is_empty() && !args.case.iter().any(|c| c == case) {
            continue;
        }
        for flavor in [Flavor::Evolvable, Flavor::Final] {
            let wanted = args.flavor.is_none_or(|f| f == flavor);
            if (flavor == Flavor::Final && !has_final) || (has_final && !wanted) {
                continue;
            }
            rows.push((case, flavor));
        }
    }

    // targets in parallel (each has its own target dir), cases one after another
    let results: Vec<Result<Vec<Image>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|&target| {
                let (rows, fw, sw_features) = (&rows, &fw, &args.sw_features);
                scope.spawn(move || {
                    rows.iter()
                        .enumerate()
                        .map(|(i, &(case, flavor))| {
                            eprintln!("[{}/{}] {} {case} {flavor:?}", i + 1, rows.len(), target.name());
                            build(fw, target, case, flavor, sw_features)
                        })
                        .collect()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("build thread panicked")).collect()
    });
    let results = results.into_iter().collect::<Result<Vec<_>>>()?;

    if let Some(n) = args.symbols {
        for (target, images) in targets.iter().zip(&results) {
            for (&(case, flavor), image) in rows.iter().zip(images) {
                println!("\n{case} ({flavor:?}), {}: .text {}, .rodata {}", target.name(), image.text, image.rodata);
                for (size, name) in image.symbols.iter().take(n) {
                    println!("{size:>7}  {name}");
                }
                let rest: u64 = image.symbols.iter().skip(n).map(|(size, _)| size).sum();
                if rest > 0 {
                    println!("{rest:>7}  ({} more)", image.symbols.len().saturating_sub(n));
                }
            }
        }
        return Ok(());
    }

    // a Markdown table: .text + .rodata in bytes
    print!("| case | types |");
    targets.iter().for_each(|t| print!(" {} |", t.name()));
    print!("\n| --- | --- |");
    targets.iter().for_each(|_| print!(" ---: |"));
    println!();
    for (i, &(case, flavor)) in rows.iter().enumerate() {
        let flavor = match (CASES.iter().any(|&(name, has_final)| name == case && has_final), flavor) {
            (false, _) => "",
            (true, Flavor::Evolvable) => "evolvable",
            (true, Flavor::Final) => "final",
        };
        print!("| {case} | {flavor} |");
        for images in &results {
            print!(" {} |", images[i].text + images[i].rodata);
        }
        println!();
    }
    Ok(())
}
