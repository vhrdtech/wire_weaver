//! Core clock cycles per request of the dispatcher (TEST-30) on the STM32H725IG (Cortex-M7, 540 MHz, I- and D-cache
//! on), counted with the DWT cycle counter: the requests of `mcu_qemu`'s `qemu-dispatch-requests`
//! (`src/dispatch_cases.rs`) to the same `flash_budget_api::BaseOnly` server. Each request runs a few times: the
//! first is the cold one, then the smallest of the others; then the reply's length and FNV hash, the same as on
//! QEMU. `cargo run --release --bin stm32h725ig-dispatch-cycles` (opt-level `z`, the shipped profile),
//! `CARGO_PROFILE_RELEASE_OPT_LEVEL=3 cargo run --release ..` for opt-level 3; prints them over defmt.
#![no_std]
#![no_main]

#[macro_use]
#[path = "../../../mcu_qemu/src/flash_budget.rs"]
mod flash_budget;
#[path = "../../../mcu_qemu/src/dispatch_cases.rs"]
mod dispatch_cases;

use core::hint::black_box;
use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use defmt::info;
use defmt_rtt as _;
use dispatch_cases::{CASES, fnv, request};
use embassy_stm32::Config;
use embassy_stm32::time::mhz;
use panic_probe as _;
use wire_weaver::prelude::*;

flash_budget_server_impl!(BaseOnly);

impl ServerState {
    base_handlers!();
}

/// Runs of each request: the cold one, then the warm ones the smallest is taken of.
const RUNS: usize = 4;

/// One request through the server, its reply into `scratch`.
#[inline(never)]
fn serve_reply<'a>(request: &[u8], scratch: &'a mut [u8]) -> Option<&'a [u8]> {
    let mut event_scratch = [0u8; 256];
    let mut server = ServerState {};
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    server
        .process_request_bytes(request, scratch, &mut out, ())
        .ok()
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

    // serve() is linked too, as in the flash bins, so this ELF's dispatcher is theirs
    black_box(serve(black_box(&[])));
    for (i, (label, _)) in CASES.iter().enumerate() {
        let mut buf = [0u8; 64];
        let req = request(i, &mut buf);
        let mut cycles = [0u32; RUNS];
        let mut reply_bytes = 0;
        let mut hash = 0;
        for c in cycles.iter_mut() {
            let mut scratch = [0u8; 256];
            let start = DWT::cycle_count();
            let reply = serve_reply(black_box(req), &mut scratch);
            *c = DWT::cycle_count().wrapping_sub(start);
            let reply = reply.unwrap_or(&[]);
            reply_bytes = reply.len();
            hash = fnv(reply);
        }
        let warm = cycles[1..].iter().copied().min().unwrap_or(0);
        info!(
            "{} | ops=1 cycles cold={} warm={} | bytes={} hash={=u32:08x}",
            label, cycles[0], warm, reply_bytes, hash
        );
    }
    info!("done");
    loop {
        cortex_m::asm::wfi();
    }
}
