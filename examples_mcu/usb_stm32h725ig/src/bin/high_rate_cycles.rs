//! Core clock cycles of high-rate observe on the STM32H725IG (Cortex-M7, 540 MHz, I- and D-cache on), counted with
//! the DWT cycle counter: a few values of a power converter observed every switching period (3 at 400 kHz and 1 MHz,
//! 2 at 2 MHz) on observe cells, the column-wise `DeltaPacked` encode, and a raw copy as the reference (the
//! `mcu_qemu` `high_rate` bench). A row changes the state, so each call runs once: the first call of a case is the
//! cold one, the smallest, largest and mean of the rest the warm ones. Then the bytes sent and the RAM of the
//! level's samples. `CARGO_PROFILE_RELEASE_OPT_LEVEL=3 cargo run --release --bin stm32h725ig-high-rate-cycles`
//! prints them over defmt.
#![no_std]
#![no_main]

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use high_rate::{Case, Run};
use panic_probe as _;

#[path = "../../../mcu_qemu/src/high_rate.rs"]
mod high_rate;
#[path = "../../../mcu_qemu/src/power_signals.rs"]
mod power_signals;

/// Cycles per run and case: calls, the first, the smallest and largest of the others, their sum.
#[derive(Clone, Copy)]
struct Stat {
    n: u32,
    first: u32,
    min: u32,
    max: u32,
    sum: u32,
}

struct Cycles([[Stat; 5]; 3]);

impl high_rate::Meter for Cycles {
    fn measure(&mut self, run: Run, case: Case, _k: usize, work: &mut dyn FnMut()) {
        let start = DWT::cycle_count();
        work();
        let cycles = DWT::cycle_count().wrapping_sub(start);
        let s = &mut self.0[run as usize][case as usize];
        if s.n == 0 {
            s.first = cycles;
        } else {
            s.min = s.min.min(cycles);
            s.max = s.max.max(cycles);
            s.sum += cycles;
        }
        s.n += 1;
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

    let empty = Stat {
        n: 0,
        first: 0,
        min: u32::MAX,
        max: 0,
        sum: 0,
    };
    let mut cycles = Cycles([[empty; 5]; 3]);
    let reports = high_rate::run(&mut cycles);
    for run in Run::ALL {
        for c in Case::ALL {
            let s = cycles.0[run as usize][c as usize];
            info!(
                "{} | {} calls={} cycles cold={} warm min={} max={} mean={}",
                run.label(),
                c.label(),
                s.n,
                s.first,
                s.min,
                s.max,
                s.sum / (s.n - 1).max(1)
            );
        }
        let r = &reports[run as usize];
        info!(
            "{} | updates={} bytes={} hash={=u32:08x} raw bytes per row={}",
            run.label(),
            r.updates,
            r.bytes,
            r.hash,
            run.raw_row_bytes()
        );
    }
    let ram = high_rate::ram();
    info!("ram three={} two={}", ram[0], ram[1]);
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
