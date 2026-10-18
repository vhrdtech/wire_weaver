//! Counts the instructions a bare-metal ELF retires between calls of its `mark_start` and `mark_end` functions,
//! running it under QEMU with one trace line per instruction (`-accel tcg,one-insn-per-tb=on -d exec,nochain`),
//! for the `mcu_qemu` / `mcu_qemu_m4f` benches (TEST-23). QEMU's trace lines carry the symbol of each instruction,
//! so no `nm` is needed.
//!
//! ```text
//! cargo run --release -- <cpu> <machine> <elf> [--profile] [--pcs=<fn>] [--max-insns=N] [--timeout=SECONDS]
//! cargo run --release -- cortex-m3 lm3s6965evb ../mcu_qemu/target/thumbv7m-none-eabi/release/qemu-series-encode-cpu-fixed-delta
//! ```
//!
//! Prints one line per `mark_start` .. `mark_end` bracket: the instructions between the two calls, minus those of
//! the first bracket (the bench's calibration pair with nothing in between). `--profile` adds, per bracket, the
//! instructions spent in each function (its own instructions only, not its callees'), most first.
//! `--pcs=<part of a function name>` adds, per bracket, how often each instruction of the functions whose name has
//! that part ran, by address: next to `llvm-objdump -d` of the ELF, where the time goes inside a function.
//!
//! The trace is read from QEMU's stderr as it comes, never written to a file (one line per instruction is
//! gigabytes for a long run). A bin that doesn't exit is stopped after `--max-insns=N` traced instructions
//! (default 200 million) or `--timeout=SECONDS` (default 600), and the run fails saying so.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Instructions traced before QEMU is stopped, unless `--max-insns=` says otherwise: a bin that never exits (a
/// fault loop, a missing semihosting exit) ends here instead of running forever.
const MAX_INSNS: u64 = 200_000_000;
/// Wall-clock seconds before QEMU is stopped, unless `--timeout=` says otherwise.
const TIMEOUT_S: u64 = 600;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let profile = args.iter().any(|a| a == "--profile");
    let pcs = args.iter().find_map(|a| a.strip_prefix("--pcs="));
    let max_insns = match args.iter().find_map(|a| a.strip_prefix("--max-insns=")) {
        Some(n) => match n.parse() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("--max-insns={n}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => MAX_INSNS,
    };
    let timeout = match args.iter().find_map(|a| a.strip_prefix("--timeout=")) {
        Some(s) => match s.parse() {
            Ok(s) => Duration::from_secs(s),
            Err(e) => {
                eprintln!("--timeout={s}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => Duration::from_secs(TIMEOUT_S),
    };
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [cpu, machine, elf] = pos[..] else {
        eprintln!(
            "usage: qemu_icount <cpu> <machine> <elf> [--profile] [--pcs=<function name part>] [--max-insns=N] \
             [--timeout=SECONDS]"
        );
        return ExitCode::FAILURE;
    };
    // no -D: QEMU writes the trace to its stderr, read here line by line as it comes, never to a file
    let child = Command::new("qemu-system-arm")
        .args(["-cpu", cpu, "-machine", machine, "-nographic"])
        .args(["-semihosting-config", "enable=on,target=native"])
        .args(["-accel", "tcg,one-insn-per-tb=on", "-d", "exec,nochain"])
        .arg("-kernel")
        .arg(elf)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("can't run qemu-system-arm: {e}");
            return ExitCode::FAILURE;
        }
    };
    let stderr = child.stderr.take().expect("piped");
    // QEMU is stopped from here when the reader gives up, so the reader runs on its own thread
    let (stop_tx, stop_rx) = mpsc::channel::<&'static str>();
    let pcs_owned = pcs.map(str::to_owned);
    let reader = std::thread::spawn(move || {
        let mut tracer = Tracer::new(pcs_owned);
        let mut traced = 0u64;
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if !line.starts_with("Trace ") {
                eprintln!("{line}");
                continue;
            }
            traced += 1;
            if traced > max_insns {
                let _ = stop_tx.send("instruction cap");
                break;
            }
            tracer.line(&line);
        }
        tracer.out
    });
    let started = Instant::now();
    let why = loop {
        if let Ok(why) = stop_rx.try_recv() {
            break Some(why);
        }
        match child.try_wait() {
            Ok(Some(_)) => break None,
            Ok(None) => {}
            Err(e) => {
                eprintln!("can't wait for qemu: {e}");
                break Some("error");
            }
        }
        if started.elapsed() > timeout {
            break Some("timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if let Some(why) = why {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        eprintln!(
            "stopped qemu ({why}: --max-insns={max_insns}, --timeout={}): the bin did not exit",
            timeout.as_secs()
        );
        return ExitCode::FAILURE;
    }
    let status = child.wait();
    let brackets = reader.join().expect("trace reader");
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("qemu exited with {s}");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("can't wait for qemu: {e}");
            return ExitCode::FAILURE;
        }
    }
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

/// Splits the trace into `mark_start` .. `mark_end` brackets, one line at a time.
struct Tracer {
    pcs: Option<String>,
    out: Vec<Bracket>,
    open: Option<Bracket>,
    prev: String,
    names: HashMap<String, String>,
}

impl Tracer {
    fn new(pcs: Option<String>) -> Self {
        Tracer {
            pcs,
            out: Vec::new(),
            open: None,
            prev: String::new(),
            names: HashMap::new(),
        }
    }

    fn line(&mut self, line: &str) {
        let Some(sym) = symbol(line) else { return };
        let name = self
            .names
            .entry(sym.to_owned())
            .or_insert_with(|| short(&format!("{:#}", rustc_demangle::demangle(sym))));
        let entered = sym != self.prev;
        if entered {
            self.prev.clear();
            self.prev.push_str(sym);
        }
        if entered && name.ends_with("::mark_start") {
            self.open = Some(Bracket::default());
            return;
        }
        if entered && name.ends_with("::mark_end") {
            self.out.extend(self.open.take());
            return;
        }
        if let Some(b) = self.open.as_mut() {
            // the rest of mark_start after its first instruction, and the call of mark_end, count as overhead
            b.count += 1;
            *b.per_fn.entry(name.clone()).or_default() += 1;
            if let (Some(part), Some(pc)) = (self.pcs.as_deref(), pc(line))
                && name.contains(part)
            {
                *b.per_pc.entry(pc).or_default() += 1;
            }
        }
    }
}
