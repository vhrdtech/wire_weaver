//! Core clock cycles of `ww_si::Fixed` against the plain fields it replaces (the `mcu_qemu` `unit_budget` bench,
//! TEST-27) on the STM32H725IG (Cortex-M7, 540 MHz, I- and D-cache on), counted with the DWT cycle counter. Each
//! case runs a few times: the first is the cold one, then the smallest of the others; then the bytes and the FNV
//! hash of each case, the same as on QEMU. `cargo run --release --bin stm32h725ig-unit-budget-cycles` (opt-level
//! `z`), `CARGO_PROFILE_RELEASE_OPT_LEVEL=3 cargo run --release ..` for opt-level 3; prints them over defmt.
#![no_std]
#![no_main]

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use panic_probe as _;
use unit_budget::{Case, RUNS};

#[path = "../../../mcu_qemu/src/unit_budget.rs"]
mod unit_budget;

/// Cycles per case and run.
struct Cycles([[u32; RUNS]; Case::ALL.len()]);

impl unit_budget::Meter for Cycles {
    fn measure(&mut self, case: Case, run: usize, work: &mut dyn FnMut()) {
        let start = DWT::cycle_count();
        work();
        self.0[case as usize][run] = DWT::cycle_count().wrapping_sub(start);
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

    let mut cycles = Cycles([[0; RUNS]; Case::ALL.len()]);
    let reports = unit_budget::run(&mut cycles);
    for case in Case::ALL {
        let c = cycles.0[case as usize];
        let warm = c[1..].iter().copied().min().unwrap_or(0);
        let r = &reports[case as usize];
        info!(
            "{} | ops={} cycles cold={} warm={} per op x10={} | bytes={} hash={=u32:08x}",
            case.label(),
            case.ops(),
            c[0],
            warm,
            warm * 10 / case.ops() as u32,
            r.bytes,
            r.hash
        );
    }
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
