//! Shared body of the `qemu-m4f-series-encode-cpu-*` bins (TEST-23): encodes the recorded `i_a` signal
//! ([super::motor_signal]) as `Series<Timed<Ampere>>` at n = 8/32/64 under one fixed [CodecPolicy], printing the
//! instructions retired per encode (QEMU `-icount shift=0` makes the semihosting `SYS_ELAPSED` tick count equal
//! to it) and the encoded length, then exits QEMU. One bin per codec set, so `cargo size` / `llvm-size` of each
//! ELF is that codec set's own flash, not every codec linked together.

use core::fmt::Write;
use cortex_m_semihosting::{debug, hio, nr, syscall1};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use wire_weaver::shrink_wrap::series::CodecPolicy;
use ww_si::{Ampere, Second, Timed};

const BATCH_SIZES: [usize; 3] = [8, 32, 64];

/// Semihosting `SYS_ELAPSED` (0x30): under `-icount shift=0` one QEMU virtual nanosecond is one retired
/// instruction, so the low 32 bits of the elapsed tick count (ample for a few thousand instructions) is an
/// instruction count.
fn elapsed_insns() -> u32 {
    unsafe { syscall1(nr::ELAPSED, 0) as u32 }
}

pub fn run(policy_name: &str, policy: CodecPolicy) -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let items: [Timed<Ampere>; 64] = core::array::from_fn(|i| {
        Timed::new(
            Second::micro(super::motor_signal::T0_US + i as u32 * super::motor_signal::DT_US),
            Ampere::milli(super::motor_signal::I_A_MILLIAMPS[i]),
        )
    });

    writeln!(stdout, "policy={policy_name}").unwrap();
    for &n in &BATCH_SIZES {
        let series = Series::with_policy(&items[..n], policy);
        let mut buf = [0u8; 512];
        let t0 = elapsed_insns();
        let encoded = series.to_ww_bytes(&mut buf).unwrap();
        let t1 = elapsed_insns();
        writeln!(
            stdout,
            "n={n} insns={} bytes={}",
            t1.wrapping_sub(t0),
            encoded.len()
        )
        .unwrap();
    }

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
