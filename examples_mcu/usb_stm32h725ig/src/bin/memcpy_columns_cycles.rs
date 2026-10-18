//! Core clock cycles of the aligned-plain column upper bound (experiment series-memcpy-fast) on the STM32H725IG
//! (Cortex-M7, 540 MHz, I- and D-cache on), counted with the DWT cycle counter: one FOC level batch (21 motor demo
//! properties + the clock, n = 32) copied from struct-of-arrays storage as little-endian payloads, bare and with a
//! real column's framing (the `mcu_qemu` `memcpy_columns` bench), two batches. Each copy runs five times: the first
//! is with cold caches, the best of the rest is the warm number. `cargo run --release --bin
//! stm32h725ig-memcpy-columns-cycles` prints them over defmt.
#![no_std]
#![no_main]

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use memcpy_columns::{BATCHES, Shape};
use panic_probe as _;

#[path = "../../../mcu_qemu/src/foc_signals.rs"]
mod foc_signals;
#[path = "../../../mcu_qemu/src/memcpy_columns.rs"]
mod memcpy_columns;

/// Warm cycles (the best of four runs after the first), cold ones (the first run) and the bytes, per batch and
/// shape.
struct Cycles {
    warm: [[u32; 2]; BATCHES],
    cold: [[u32; 2]; BATCHES],
    bytes: [[usize; 2]; BATCHES],
}

impl memcpy_columns::Meter for Cycles {
    fn measure(&mut self, shape: Shape, batch: usize, work: &mut dyn FnMut() -> usize) {
        let (mut cold, mut warm, mut bytes) = (0, u32::MAX, 0);
        for k in 0..5 {
            let start = DWT::cycle_count();
            bytes = work();
            let cycles = DWT::cycle_count().wrapping_sub(start);
            if k == 0 {
                cold = cycles;
            } else {
                warm = warm.min(cycles);
            }
        }
        let s = shape as usize;
        self.warm[batch][s] = warm;
        self.cold[batch][s] = cold;
        self.bytes[batch][s] = bytes;
    }
}

fn measure() {
    let mut c = Cycles {
        warm: [[0; 2]; BATCHES],
        cold: [[0; 2]; BATCHES],
        bytes: [[0; 2]; BATCHES],
    };
    memcpy_columns::run(&mut c);
    for batch in 0..BATCHES {
        for shape in Shape::ALL {
            let s = shape as usize;
            info!(
                "batch={} {} bytes={} cycles cold={} warm={}",
                batch,
                shape.label(),
                c.bytes[batch][s],
                c.cold[batch][s],
                c.warm[batch][s]
            );
        }
    }
}

#[entry]
fn main() -> ! {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = None;
        config.rcc.csi = false;
        config.rcc.hse = Some(Hse {
            freq: mhz(24),
            mode: HseMode::Bypass,
        });
        config.rcc.hsi48 = None;
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV2,
            mul: PllMul::MUL45,
            divp: Some(PllDiv::DIV1),
            divq: Some(PllDiv::DIV4),
            divr: Some(PllDiv::DIV2),
        });
        config.rcc.sys = Sysclk::PLL1_P; // 540MHz
        config.rcc.d1c_pre = AHBPrescaler::DIV2;
        config.rcc.ahb_pre = AHBPrescaler::DIV2;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.apb3_pre = APBPrescaler::DIV2;
        config.rcc.apb4_pre = APBPrescaler::DIV2;
        config.rcc.voltage_scale = VoltageScale::Scale0;
        config.rcc.supply_config = SupplyConfig::DirectSMPS;
    }
    let _p = embassy_stm32::init(config);
    let mut cp = cortex_m::Peripherals::take().unwrap();
    cp.SCB.enable_icache();
    cp.SCB.enable_dcache(&mut cp.CPUID);
    cp.DCB.enable_trace();
    cp.DWT.enable_cycle_counter();

    measure();
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
