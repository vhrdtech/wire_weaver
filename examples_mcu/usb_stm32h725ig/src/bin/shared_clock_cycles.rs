//! Core clock cycles of the shared clock measurement (idea I-2) on the STM32H725IG (Cortex-M7, 540 MHz, I- and
//! D-cache on), counted with the DWT cycle counter: one FOC level batch of 32 samples of all 21 motor demo
//! properties as 21 x `Series<Timed<T>>` against 21 x `Series<T>` + one `Series<Second>` (the `mcu_qemu`
//! `shared_clock` bench), two batches, under `DeltaPacked` and `RawPlain`. Each encode runs five times: the first
//! is with cold caches, the best of the rest is the warm number. `cargo run --release --bin
//! stm32h725ig-shared-clock-cycles` prints them over defmt.
#![no_std]
#![no_main]

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use panic_probe as _;
use shared_clock::{BATCHES, Group};
use wire_weaver::shrink_wrap::series::{DeltaPacked, RawPlain, SeriesPolicy};

#[path = "../../../mcu_qemu/src/foc_signals.rs"]
mod foc_signals;
#[path = "../../../mcu_qemu/src/shared_clock.rs"]
mod shared_clock;

/// Warm cycles (the best of four runs after the first) and cold ones (the first run) summed per batch and group,
/// and the bytes.
struct Cycles {
    warm: [[u32; 4]; BATCHES],
    cold: [[u32; 4]; BATCHES],
    bytes: [[usize; 4]; BATCHES],
}

impl shared_clock::Meter for Cycles {
    fn measure(&mut self, group: Group, batch: usize, work: &mut dyn FnMut() -> usize) {
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
        let g = group as usize;
        self.warm[batch][g] += warm;
        self.cold[batch][g] += cold;
        self.bytes[batch][g] += bytes;
    }
}

fn measure<P: SeriesPolicy + Copy>(name: &str, policy: P) {
    let mut c = Cycles {
        warm: [[0; 4]; BATCHES],
        cold: [[0; 4]; BATCHES],
        bytes: [[0; 4]; BATCHES],
    };
    shared_clock::run(policy, &mut c);
    for batch in 0..BATCHES {
        for group in Group::ALL {
            let g = group as usize;
            info!(
                "{} batch={} {} bytes={} cycles cold={} warm={}",
                name,
                batch,
                group.label(),
                c.bytes[batch][g],
                c.cold[batch][g],
                c.warm[batch][g]
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

    measure("DeltaPacked", DeltaPacked);
    measure("RawPlain", RawPlain);
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
