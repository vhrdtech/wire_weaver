//! Core clock cycles of a `Series<Timed<Ampere>>` encode on the STM32H725IG (Cortex-M7, 540 MHz, I- and D-cache
//! on), counted with the DWT cycle counter (SW-63): the recorded motor demo `i_a` signal of the `mcu_qemu` benches
//! at n = 8/32/64 under `CodecPolicy::SMALLEST`, `fixed(delta,packed)`, `fixed(delta-of-delta,packed)` and the
//! compile-time `DeltaPacked`. Each encode runs five times: the first is with cold caches, the best of the rest is
//! the warm number. `cargo run --release --bin stm32h725ig-series-encode-cycles` prints them over defmt.
#![no_std]
#![no_main]

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use panic_probe as _;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use wire_weaver::shrink_wrap::series::{
    CodecPolicy, Coder, DeltaPacked, FloatCodec, NumericCodec, Predictor, SeriesPolicy,
};
use ww_si::{Ampere, Second, Timed};

#[path = "../../../mcu_qemu/src/motor_signal.rs"]
mod motor_signal;

fn items() -> [Timed<Ampere>; 64] {
    core::array::from_fn(|i| {
        Timed::new(
            Second::micro(motor_signal::T0_US + i as u32 * motor_signal::DT_US),
            Ampere::milli(motor_signal::I_A_MILLIAMPS[i]),
        )
    })
}

fn measure<P: SeriesPolicy + Copy>(name: &str, policy: P, items: &[Timed<Ampere>]) {
    for n in [8, 32, 64] {
        let series = Series::with_policy(&items[..n], policy);
        let mut buf = [0u8; 512];
        let (mut cold, mut warm, mut bytes) = (0, u32::MAX, 0);
        for k in 0..5 {
            let start = DWT::cycle_count();
            let len = series.to_ww_bytes(&mut buf).map(|b| b.len());
            let cycles = DWT::cycle_count().wrapping_sub(start);
            bytes = len.unwrap_or(0);
            if k == 0 {
                cold = cycles;
            } else {
                warm = warm.min(cycles);
            }
        }
        info!(
            "{} n={} bytes={} cycles cold={} warm={}",
            name, n, bytes, cold, warm
        );
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

    let items = items();
    measure("SMALLEST", CodecPolicy::SMALLEST, &items);
    let fixed = |p| CodecPolicy::fixed(NumericCodec::new(p, Coder::Packed), FloatCodec::Xor);
    measure("fixed(delta,packed)", fixed(Predictor::Delta), &items);
    measure(
        "fixed(delta-of-delta,packed)",
        fixed(Predictor::DeltaOfDelta),
        &items,
    );
    measure("DeltaPacked", DeltaPacked, &items);
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
