//! The bit I/O bench of `examples_mcu/mcu_qemu` (`qemu-bit-io`) on the host: the same cases, from the same source
//! (`bit_io.rs`), timed instead of counted. `cargo bench -p series --bench bit_io` prints nanoseconds per operation,
//! the best of many batches, and the FNV hash of each case's bytes and values (the same as on the MCU when the
//! writer and the reader are right). Wall-clock numbers: compare two builds on the same idle PC (SW-74).

#[path = "../../../examples_mcu/mcu_qemu/src/bit_io.rs"]
mod bit_io;
#[path = "../../../examples_mcu/mcu_qemu/src/motor_signal.rs"]
#[allow(dead_code)]
mod motor_signal;

use bit_io::{Case, Meter};
use std::time::Instant;

/// Calls of a case per batch, and batches per run: the best batch counts.
const CALLS: u32 = 2_000;
const BATCHES: u32 = 30;

struct Timer {
    best_ns: [f64; Case::ALL.len()],
    calls: u32,
    batches: u32,
}

impl Meter for Timer {
    fn measure(&mut self, case: Case, _run: usize, work: &mut dyn FnMut()) {
        let i = Case::ALL.iter().position(|c| *c == case).unwrap();
        for _ in 0..self.batches {
            let start = Instant::now();
            for _ in 0..self.calls {
                work();
            }
            let ns = start.elapsed().as_nanos() as f64 / self.calls as f64 / case.ops() as f64;
            if ns < self.best_ns[i] {
                self.best_ns[i] = ns;
            }
        }
    }
}

fn main() {
    // libtest's arguments, as a test runner passes them (nextest with `--all-targets`, as CI runs it): `--list`
    // names one benchmark and nothing ignored; a run without `--bench` is a smoke run, every case once, as libtest
    // runs a benchmark in test mode.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--list") {
        if !args.iter().any(|a| a == "--ignored") {
            println!("bit_io: benchmark");
        }
        return;
    }
    let bench = args.iter().any(|a| a == "--bench");
    let mut timer = Timer {
        best_ns: [f64::MAX; Case::ALL.len()],
        calls: if bench { CALLS } else { 1 },
        batches: if bench { BATCHES } else { 1 },
    };
    let reports = bit_io::run(&mut timer);
    println!(
        "{:<34} {:>10} {:>6} {:>10}",
        "case", "ns/op", "bytes", "hash"
    );
    for (i, case) in Case::ALL.iter().enumerate() {
        println!(
            "{:<34} {:>10.2} {:>6} {:>10x}",
            case.label(),
            timer.best_ns[i],
            reports[i].bytes,
            reports[i].hash
        );
    }
}
