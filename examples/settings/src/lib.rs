//! Two settings kept across a reboot by `ww_settings`, on a simulated flash, over the real client/server codepath
//! (`docs/api/settings.md`): [Device] holds `meter_window_ms` / `led_on_at_boot` as plain fields,
//! written directly by the generated `value_on_changed` property setters; `changed_<name>` tells [struct@Settings]
//! about the change, and [serve]'s periodic tick writes it once the [Debounce] is over. `tests/reboot.rs` connects a
//! [Client], sets both, waits past the debounce, stops the device, and reopens it from the same flash image.

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, Instant};

use wire_weaver::prelude::*;
use wire_weaver::ww_version::FullVersion;
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::in_process::{DeviceRx, DeviceTx, TokioClock};
use wire_weaver_client::{ClientConfig, Commander, WwClient};
pub use ww_settings::Debounce;
use ww_settings::fake_flash::FakeFlash;
use ww_settings::{Key, Settings, Store};

use settings_api::UserSettings;

/// Flash word matches the STM32G0 the `nucleo_g0b1re` settings bin uses (8 bytes, ECC); page size is this demo's
/// own, not a real part's.
pub type Flash = FakeFlash<8, 4096>;
/// 2 pages: the minimum a [Store] accepts.
pub const PAGES: usize = 2;
pub const RANGE: core::ops::Range<u32> = 0..(PAGES * 4096) as u32;
/// Longest message either side sends or receives.
const MAX_MESSAGE_LEN: usize = 256;

/// Server side: a plain field per `value_on_changed` property, backed by a [struct@Settings] group written to a
/// [Store].
pub struct Device {
    /// Averaging window of a sensor reading, milliseconds.
    pub meter_window_ms: u16,
    /// Whether the user LED lights up at boot.
    pub led_on_at_boot: bool,
    settings: Settings<UserSettings, 64>,
    store: Store<Flash, 256>,
    started: Instant,
}

impl Device {
    /// Loads `UserSettings` from `flash`'s [Key::USER] record, every field at its default when there is none.
    pub async fn new(flash: Flash, debounce: Debounce) -> Self {
        let mut store = Store::new(flash, RANGE).expect("RANGE is page aligned, at least 2 pages");
        let mut settings: Settings<UserSettings, 64> = Settings::new(Key::USER, debounce);
        settings
            .load(&mut store)
            .await
            .expect("fake flash read does not fail");
        let value = settings.get().clone();
        Device {
            meter_window_ms: value.meter_window_ms,
            led_on_at_boot: value.led_on_at_boot,
            settings,
            store,
            started: Instant::now(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Write the current value if its [Debounce] is over since the last change; [serve] calls this on every tick.
    pub async fn persist_if_due(&mut self) {
        let now = self.now_ms();
        self.settings
            .persist_if_due(&mut self.store, now)
            .await
            .expect("fake flash write does not fail");
    }

    async fn changed_meter_window_ms(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        let value = self.meter_window_ms;
        let now = self.now_ms();
        self.settings.set(now, |s| s.meter_window_ms = value);
    }

    async fn changed_led_on_at_boot(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        let value = self.led_on_at_boot;
        let now = self.now_ms();
        self.settings.set(now, |s| s.led_on_at_boot = value);
    }

    /// The flash, to reopen with [Device::new] after a simulated reboot.
    pub fn into_flash(self) -> Flash {
        self.store.into_flash()
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        settings_api :: SettingsDemo for super::Device,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "meter_window_ms=value_on_changed, led_on_at_boot=value_on_changed",
    );
}

struct Backend {
    device: Device,
    version: FullVersion<'static>,
}

impl WireWeaverAsyncApiBackend for Backend {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.device
            .process_request_bytes(data, scratch, out, medium)
            .await
    }

    fn version(&self) -> FullVersion<'_> {
        self.version
    }
}

/// Run `device` under the path it was registered at ([wire_weaver_client::in_process::device]), persisting once a
/// change's debounce is over, until `stop` resolves. `ww_device` futures are not `Send`: run this on its own
/// thread with a current-thread runtime, as the [wire_weaver_client::in_process] module doc shows.
pub async fn serve(
    device: Device,
    tx: DeviceTx,
    rx: DeviceRx,
    stop: impl Future<Output = ()>,
) -> Device {
    let mut stop = pin!(stop);
    let mut scratch = [0u8; MAX_MESSAGE_LEN];
    let mut event_scratch = [0u8; MAX_MESSAGE_LEN];
    let config = ww_device::LinkConfig::new(
        settings_api::SETTINGS_DEMO_FULL_GID,
        api_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let mut server = ww_device::Server::new(
        config,
        tx,
        rx,
        TokioClock::new(),
        &mut scratch,
        &mut event_scratch,
    );
    let mut backend = Backend {
        device,
        version: settings_api::SETTINGS_DEMO_FULL_GID,
    };
    let mut tick = tokio::time::interval(Duration::from_millis(5));
    loop {
        tokio::select! {
            ready = server.wait() => {
                server.handle(ready, &mut backend).await;
            }
            // also while no host is connected: a change made just before it left must still reach the flash
            _ = tick.tick() => backend.device.persist_if_due().await,
            () = &mut stop => break,
        }
    }
    backend.device
}

/// Host side: the generated `read_meter_window_ms` / `write_led_on_at_boot` / ... live on this through
/// [wire_weaver::ww_codegen!] below.
pub struct Client {
    cmd: Commander,
}

impl WwClient for Client {
    fn default_config() -> ClientConfig {
        ClientConfig::default()
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        settings_api :: SettingsDemo for super::Client,
        client = "std_client",
    );
}
