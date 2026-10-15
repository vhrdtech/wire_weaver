//! `settings_api::SettingsDemo` over RTT (next to defmt logging on another RTT channel, same layout as
//! `ww_rtt.rs`), `meter_window_ms` / `led_on_at_boot` kept in the last 4 pages of bank 2 (`memory.x`) through
//! `ww_settings`. `led_on_at_boot` also drives the board's user LED live, so a change is visible without a
//! reboot.
//!
//! Run with `just run nucleo settings`, then stop `probe-rs run` (Ctrl+C, the firmware keeps running) and connect
//! `examples/settings`'s `Client` over RTT (`Client::config(|c| c.rtt("STM32G0B1RETx", None)).connect()`).
//!
//! A demo, not a product: it reads the flash with embassy-stm32's plain driver. On this part's ECC flash a power
//! cut while a word is written leaves a word that faults when read, so the next boot hangs in the fault handler;
//! a device needs the ECC-aware driver of `mcu/ww_ecc_flash` (SRV-19), see the settings store docs page.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::{Debug2Format, error, info};
use embassy_stm32::flash::{Async, Bank2Region};
use embassy_stm32::{
    Config, bind_interrupts,
    flash::{self, Flash},
    gpio::{Level, Output, Speed},
};
use panic_probe as _;
use rtt_target::{ChannelMode::NoBlockSkip, rtt_init};
use settings_api::UserSettings;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_device::rtt::{RttBuffers, RttConfig, rtt_server};
use ww_device::{EmbassyClock, LinkConfig};
use ww_settings::{Debounce, Key, Persisted, Settings, Store};

/// Maximum WireWeaver message length, RTT channels below must be able to hold a whole message
/// plus its framing (see `ww_device::rtt`), and should be at least that size for `NoBlockSkip`.
const MAX_MESSAGE_LEN: usize = 1024;
static RTT_BUFFERS: StaticCell<RttBuffers<MAX_MESSAGE_LEN>> = StaticCell::new();

bind_interrupts!(struct Irqs {
    FLASH => flash::InterruptHandler;
});

/// 4 pages (2K each) at the end of bank 2 reserved in `memory.x`.
const PAGES: u32 = 4;
const PAGE_SIZE: u32 = 2048;

struct ServerState {
    led: Output<'static>,
    meter_window_ms: u16,
    led_on_at_boot: bool,
    settings: Settings<UserSettings, 64>,
    store: Store<Bank2Region<'static, Async>, 256>,
}

impl WireWeaverAsyncApiBackend for ServerState {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> FullVersion<'_> {
        settings_api::SETTINGS_DEMO_FULL_GID
    }
}

mod server_impl {
    wire_weaver::ww_codegen!(
        settings_api :: SettingsDemo for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "meter_window_ms=value_on_changed, led_on_at_boot=value_on_changed",
        introspect = "no_docs",
    );
}

impl ServerState {
    /// `now_ms` is a free-running millisecond clock (`embassy_time::Instant::now()`), good enough for the
    /// [Debounce] window.
    fn now_ms(&self) -> u64 {
        embassy_time::Instant::now().as_millis()
    }

    async fn changed_meter_window_ms(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        let value = self.meter_window_ms;
        info!("meter_window_ms changed: {}", value);
        let now = self.now_ms();
        self.settings.set(now, |s| s.meter_window_ms = value);
    }

    async fn changed_led_on_at_boot(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        let value = self.led_on_at_boot;
        info!("led_on_at_boot changed: {}", value);
        if value {
            self.led.set_high();
        } else {
            self.led.set_low();
        }
        let now = self.now_ms();
        self.settings.set(now, |s| s.led_on_at_boot = value);
    }

    /// Write the current value if its debounce is over, then erase the page the next compaction moves to (if it
    /// needs it), so the slow erase happens now and not inside a later write; the main loop's periodic tick calls this.
    async fn persist_if_due(&mut self) {
        let now = self.now_ms();
        match self.settings.persist_if_due(&mut self.store, now).await {
            Ok(Some(Persisted::Written(_))) => {
                if let Err(e) = self.store.maintain().await {
                    error!("settings erase ahead failed: {:?}", Debug2Format(&e));
                }
            }
            Ok(_) => {}
            Err(e) => error!("settings persist failed: {:?}", Debug2Format(&e)),
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let channels = rtt_init! {
        up: {
            0: {
                size: 1024,
                mode: NoBlockSkip,
                name: "defmt"
            }
            1: {
                size: 2048,
                mode: NoBlockSkip,
                name: "ww_up"
            }
        }
        down: {
            0: {
                size: 2048,
                mode: NoBlockSkip,
                name: "ww_down"
            }
        }
    };
    rtt_target::set_defmt_channel(channels.up.0);
    info!("WireWeaver settings demo on STM32G0B1 starting...");

    let p = embassy_stm32::init(Config::default());

    #[cfg(feature = "b129a_cannify")]
    let led = Output::new(p.PB14, Level::Low, Speed::Low);
    #[cfg(feature = "nucleo_g0b1re")]
    let led = Output::new(p.PA5, Level::Low, Speed::Low);

    let flash = Flash::new(p.FLASH, Irqs);
    let bank2 = flash.into_regions().bank2_region;
    let range = (bank2.0.size - PAGE_SIZE * PAGES)..bank2.0.size;
    let mut store: Store<_, 256> =
        Store::new(bank2, range).expect("range is page aligned, at least 2 pages");
    let mut settings: Settings<UserSettings, 64> = Settings::new(
        Key::USER,
        Debounce {
            quiet_ms: 1_000,
            max_ms: 10_000,
        },
    );
    let report = settings
        .load(&mut store)
        .await
        .expect("flash read does not fail");
    info!("settings loaded: {:?}", Debug2Format(&report));
    let value = settings.get().clone();

    let mut led = led;
    if value.led_on_at_boot {
        led.set_high();
    }
    let mut state = ServerState {
        led,
        meter_window_ms: value.meter_window_ms,
        led_on_at_boot: value.led_on_at_boot,
        settings,
        store,
    };

    let link_config = LinkConfig::new(
        settings_api::SETTINGS_DEMO_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let mut server = rtt_server(
        link_config,
        channels.up.1,
        channels.down.0,
        EmbassyClock,
        RttConfig::default(),
        RTT_BUFFERS.init(RttBuffers::new()),
    );
    info!("init done");
    let mut tick = embassy_time::Ticker::every(embassy_time::Duration::from_millis(50));
    loop {
        match embassy_futures::select::select(server.wait(), tick.next()).await {
            embassy_futures::select::Either::First(ready) => {
                server.handle(ready, &mut state).await;
            }
            // also while no host is connected: a change made just before it left must still reach the flash
            embassy_futures::select::Either::Second(()) => state.persist_if_due().await,
        }
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    cnt::cnt!(unhandled_exceptions: u32, warn);
    error!("Unhandled exception (IRQn = {})", irqn);
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    error!("HardFault {}", Debug2Format(ef));

    loop {}
}
