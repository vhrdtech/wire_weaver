//! Counts the instructions a bare-metal ELF retires between calls of its `mark_start` and `mark_end` functions,
//! running it under QEMU with one trace line per instruction (`-accel tcg,one-insn-per-tb=on -d exec,nochain`),
//! for the `mcu_qemu` / `mcu_qemu_m4f` benches (TEST-23). QEMU's trace lines carry the symbol of each instruction,
//! so no `nm` is needed.
//!
//! ```text
//! cargo run --release -- <cpu> <machine> <elf> [--profile]
//! cargo run --release -- cortex-m3 lm3s6965evb ../mcu_qemu/target/thumbv7m-none-eabi/release/qemu-series-encode-cpu-fixed-delta
//! ```
//!
//! Prints one line per `mark_start` .. `mark_end` bracket: the instructions between the two calls, minus those of
//! the first bracket (the bench's calibration pair with nothing in between). `--profile` adds, per bracket, the
//! instructions spent in each function (its own instructions only, not its callees'), most first.
//! `--pcs=<part of a function name>` adds, per bracket, how often each instruction of the functions whose name has
//! that part ran, by address: next to `llvm-objdump -d` of the ELF, where the time goes inside a function.

use std::collections::HashMap;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let profile = args.iter().any(|a| a == "--profile");
    let pcs = args.iter().find_map(|a| a.strip_prefix("--pcs="));
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [cpu, machine, elf] = pos[..] else {
        eprintln!("usage: qemu_icount <cpu> <machine> <elf> [--profile] [--pcs=<function name part>]");
        return ExitCode::FAILURE;
    };
    let trace = std::env::temp_dir().join(format!("qemu_icount_{}.log", std::process::id()));
    let status = Command::new("qemu-system-arm")
        .args(["-cpu", cpu, "-machine", machine, "-nographic"])
        .args(["-semihosting-config", "enable=on,target=native"])
        .args([
            "-accel",
            "tcg,one-insn-per-tb=on",
            "-d",
            "exec,nochain",
            "-D",
        ])
        .arg(&trace)
        .arg("-kernel")
        .arg(elf)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("qemu exited with {s}");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("can't run qemu-system-arm: {e}");
            return ExitCode::FAILURE;
        }
    }
    let text = std::fs::read_to_string(&trace);
    let _ = std::fs::remove_file(&trace);
    let text = match text {
        Ok(t) => t,
        Err(e) => {
            eprintln!("can't read the trace: {e}");
            return ExitCode::FAILURE;
        }
    };
    let brackets = brackets(&text, pcs);
    let Some(calibration) = brackets.first().map(|b| b.count) else {
        eprintln!("no mark_start .. mark_end bracket in the trace");
        return ExitCode::FAILURE;
    };
    for (i, b) in brackets.iter().enumerate().skip(1) {
        println!("bracket {i}: {} instructions", b.count - calibration);
        if profile {
            let mut fns: Vec<_> = b.per_fn.iter().collect();
            fns.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (name, n) in fns {
                println!("  {n:>8}  {name}");
            }
        }
        if pcs.is_some() {
            let mut at: Vec<_> = b.per_pc.iter().collect();
            at.sort();
            for (pc, n) in at {
                println!("  {pc:>8x}  {n}");
            }
        }
    }
    ExitCode::SUCCESS
}

#[derive(Default)]
struct Bracket {
    count: usize,
    per_fn: HashMap<String, usize>,
    per_pc: HashMap<u64, usize>,
}

/// The symbol of a trace line, `Trace 0: 0x.. [../pc/../..] symbol`, demangled without its hash.
fn symbol(line: &str) -> Option<&str> {
    let (_, sym) = line.split_once("] ")?;
    Some(sym.trim())
}

/// The address of a trace line's instruction, the second field in the brackets.
fn pc(line: &str) -> Option<u64> {
    let (_, rest) = line.split_once('[')?;
    let field = rest.split('/').nth(1)?;
    u64::from_str_radix(field, 16).ok()
}

/// `name` with generic arguments nested two deep or more left out (`<..>`), so a profile line stays readable.
fn short(name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in name.chars() {
        match c {
            '<' => {
                depth += 1;
                match depth {
                    1 => out.push('<'),
                    2 => out.push_str("<..>"),
                    _ => {}
                }
            }
            '>' => {
                if depth == 1 {
                    out.push('>');
                }
                depth = depth.saturating_sub(1);
            }
            _ if depth <= 1 => out.push(c),
            _ => {}
        }
    }
    out
}

fn brackets(text: &str, pcs: Option<&str>) -> Vec<Bracket> {
    let mut out = Vec::new();
    let mut open: Option<Bracket> = None;
    let mut prev = "";
    let mut names: HashMap<&str, String> = HashMap::new();
    for line in text.lines().filter(|l| l.starts_with("Trace ")) {
        let Some(sym) = symbol(line) else { continue };
        let name = names
            .entry(sym)
            .or_insert_with(|| short(&format!("{:#}", rustc_demangle::demangle(sym))));
        let entered = sym != prev;
        prev = sym;
        if entered && name.ends_with("::mark_start") {
            open = Some(Bracket::default());
            continue;
        }
        if entered && name.ends_with("::mark_end") {
            out.extend(open.take());
            continue;
        }
        if let Some(b) = open.as_mut() {
            // the rest of mark_start after its first instruction, and the call of mark_end, count as overhead
            b.count += 1;
            *b.per_fn.entry(name.clone()).or_default() += 1;
            if let (Some(part), Some(pc)) = (pcs, pc(line))
                && name.contains(part)
            {
                *b.per_pc.entry(pc).or_default() += 1;
            }
        }
    }
    out
}
