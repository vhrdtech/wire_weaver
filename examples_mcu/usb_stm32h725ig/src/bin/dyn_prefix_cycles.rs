//! Core clock cycles of the dynamic-prefix bench (`mcu_qemu`'s `dyn_prefix`) on the STM32H725IG (Cortex-M7, 540 MHz,
//! I- and D-cache on), counted with the DWT cycle counter: one FOC level batch of 32 samples of all 21 motor demo
//! properties as `Series<Quantity<U>>` (a prefix and a number type per item) against the same numbers as
//! `Series<Fixed<U, P, i32>>`, by the item walk and from a `SeriesStore`, the level's clock as `Series<Second>`
//! against `Series<DeviceMicros<u32>>`, and the quantities autoranged (the prefix changes mid-batch), two batches,
//! `DeltaPacked`. Each encode runs five times: the first is with cold caches, the best of the rest is the warm
//! number. `cargo run --release --bin stm32h725ig-dyn-prefix-cycles` prints them over defmt.
#![no_std]
#![no_main]

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use dyn_prefix::{BATCHES, Group};
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use panic_probe as _;
use wire_weaver::shrink_wrap::series::DeltaPacked;

#[path = "../../../mcu_qemu/src/dyn_prefix.rs"]
mod dyn_prefix;
#[path = "../../../mcu_qemu/src/foc_signals.rs"]
mod foc_signals;

/// Warm cycles (the best of four runs after the first) and cold ones (the first run) summed per batch and group,
/// and the bytes.
struct Cycles {
    warm: [[u32; Group::COUNT]; BATCHES],
    cold: [[u32; Group::COUNT]; BATCHES],
    bytes: [[usize; Group::COUNT]; BATCHES],
    changes: [(usize, usize); BATCHES],
}

impl dyn_prefix::Meter for Cycles {
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

    fn changes(&mut self, batch: usize, columns: usize, changes: usize) {
        self.changes[batch] = (columns, changes);
    }
}

impl Cycles {
    fn new() -> Self {
        Cycles {
            warm: [[0; Group::COUNT]; BATCHES],
            cold: [[0; Group::COUNT]; BATCHES],
            bytes: [[0; Group::COUNT]; BATCHES],
            changes: [(0, 0); BATCHES],
        }
    }

    fn print(&self, groups: &[Group]) {
        for batch in 0..BATCHES {
            for &group in groups {
                let g = group as usize;
                info!(
                    "batch={} {} bytes={} cycles cold={} warm={}",
                    batch,
                    group.label(),
                    self.bytes[batch][g],
                    self.cold[batch][g],
                    self.warm[batch][g]
                );
            }
            if groups.contains(&Group::Auto) {
                let (columns, changes) = self.changes[batch];
                info!(
                    "batch={} prefix changes: {} in {} columns",
                    batch, changes, columns
                );
            }
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

    let mut quantity = Cycles::new();
    dyn_prefix::run_quantity(DeltaPacked, &mut quantity);
    quantity.print(&Group::QUANTITY);
    let mut fixed = Cycles::new();
    dyn_prefix::run_fixed(DeltaPacked, &mut fixed);
    fixed.print(&Group::FIXED);
    let mut auto = Cycles::new();
    dyn_prefix::run_auto(DeltaPacked, &mut auto);
    auto.print(&Group::AUTO);
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
