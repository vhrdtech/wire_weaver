//! `settings_api::SettingsDemo` over USB (same transport as `blinky.rs`), `meter_window_ms` / `led_on_at_boot`
//! kept in the last 2 sectors (128K each) of bank 2 (`memory.x`) through `ww_settings`. `led_on_at_boot` also
//! drives the board's user LED live, so a change is visible without a reboot.
//!
//! `cargo run --bin nucleo-h743zi2-settings --features settings`
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::flash::{Async, Bank2Region};
use embassy_stm32::{
    Config, bind_interrupts,
    flash::{self, Flash},
    gpio::{Level, Output, Speed},
    peripherals::USB_OTG_FS,
    usb,
    usb::Driver,
};
use panic_probe as _;
use settings_api::UserSettings;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver_usb_embassy::{LinkConfig, UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};
use ww_settings::{Debounce, Key, Settings, Store};

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<USB_OTG_FS>;
    FLASH => flash::InterruptHandler;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

/// 2 sectors (128K each) at the end of bank 2 reserved in `memory.x`.
const SECTORS: u32 = 2;
const SECTOR_SIZE: u32 = 128 * 1024;
/// How often the main loop checks whether a changed value's debounce is over.
const TICK: embassy_time::Duration = embassy_time::Duration::from_millis(50);

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, Driver<'static, USB_OTG_FS>>,
    mut state: ServerState,
) {
    let mut tick = embassy_time::Ticker::every(TICK);
    loop {
        match embassy_futures::select::select(server.wait(), tick.next()).await {
            embassy_futures::select::Either::First(ready) => {
                server.handle(ready, &mut state).await;
            }
            embassy_futures::select::Either::Second(()) => {
                if server.is_up() {
                    state.persist_if_due().await;
                }
            }
        }
    }
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

struct ServerState {
    led: Output<'static>,
    meter_window_ms: u16,
    led_on_at_boot: bool,
    settings: Settings<UserSettings, 64>,
    store: Store<Bank2Region<'static, Async>, 256>,
}

mod server_impl {
    wire_weaver::ww_codegen!(
        settings_api :: SettingsDemo for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "meter_window_ms=value_on_changed, led_on_at_boot=value_on_changed",
        introspect = "with_docs",
    );
}

impl ServerState {
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

    /// Write the current value if its debounce is over; `ww_server_task`'s periodic tick calls this.
    async fn persist_if_due(&mut self) {
        let now = self.now_ms();
        match self.settings.persist_if_due(&mut self.store, now).await {
            Ok(_) => {}
            Err(e) => error!("settings persist failed: {:?}", Debug2Format(&e)),
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("WireWeaver settings demo on Nucleo H743ZI2 is starting...");

    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = Some(HSIPrescaler::DIV1);
        config.rcc.csi = true;
        config.rcc.hsi48 = Some(Hsi48Config {
            sync_from_usb: true,
        }); // needed for USB
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSI,
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL50,
            fracn: None,
            divp: Some(PllDiv::DIV2),
            divq: None,
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P; // 400 Mhz
        config.rcc.ahb_pre = AHBPrescaler::DIV2; // 200 Mhz
        config.rcc.apb1_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb2_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb3_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb4_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.voltage_scale = VoltageScale::Scale1;
        config.rcc.mux.usbsel = mux::Usbsel::HSI48;
    }
    let p = embassy_stm32::init(config);
    info!("RCC and RAM init done");

    let led = Output::new(p.PE1, Level::Low, Speed::Low);

    let flash = Flash::new(p.FLASH, Irqs);
    let bank2 = flash.into_regions().bank2_region;
    let range = (bank2.0.size - SECTOR_SIZE * SECTORS)..bank2.0.size;
    let mut store: Store<_, 256> =
        Store::new(bank2, range).expect("range is sector aligned, at least 2 sectors");
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
    let state = ServerState {
        led,
        meter_window_ms: value.meter_window_ms,
        led_on_at_boot: value.led_on_at_boot,
        settings,
        store,
    };

    static EP_OUT_BUF: StaticCell<[u8; EP_OUT_BUF_LEN]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUF.init([0u8; EP_OUT_BUF_LEN]);
    let usb_config = usb::Config::default();
    let driver = Driver::new_fs(
        p.USB_OTG_FS,
        Irqs,
        p.PA12,
        p.PA11,
        ep_out_buffer,
        usb_config,
    );
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let link_config = LinkConfig::new(
        settings_api::SETTINGS_DEMO_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::fs_higher_speed(),
        link_config,
        server_impl::API_ID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(ww_server_task(server, state)));

    info!("init done");
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
