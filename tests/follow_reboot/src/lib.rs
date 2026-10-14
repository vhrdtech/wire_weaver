//! CLT-21 end to end without hardware: a simulated rePort-like RP2350 device whose app reboots into the boot ROM
//! (`ww_dfu::BootControl::reboot(Updater)`), comes back as a simulated BOOTSEL device under the same serial number
//! (the chip id), takes a new image over PICOBOOT and boots it.
//!
//! - The app is the `dfu_lean_app_api` server (`UpdateNode` + `BootControl`) on `ww_device::Server`, over the
//!   simulated USB of `wire_weaver_client::in_process` at the path of its serial number. `UpdateNode::node` reports
//!   the version of the image in flash.
//! - The boot ROM is [ww_picoboot::sim::SimRp2350], holding the flash; the app boots from it, so what the host wrote
//!   is what runs next.
//! - [SimBus] is what the host lists: the app or the BOOTSEL device, as a real USB bus would show them, with a short
//!   gap while the device re-enumerates.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use wire_weaver::prelude::*;
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::follow::Bus;
use wire_weaver_client::in_process::{self, TokioClock};
use wire_weaver_client::{ApiInfo, DeviceInfo, UsbLocation};
use ww_device::{DisconnectReason, LinkConfig};
use ww_dfu::{BootState, BootTarget, Error, LastResult, NodeInfo, Offers, Outcome, Profile, Role};
use ww_picoboot::proto::{
    FLASH_BASE, REBOOT2_TYPE_BOOTSEL, REBOOT2_TYPE_MASK, RP2350_BOOTSEL_PID, RPI_VID,
};
use ww_picoboot::sim::SimRp2350;

/// VID:PID of the simulated app.
pub const APP_VID: u16 = 0xc0de;
pub const APP_PID: u16 = 0xcafe;
const MAX_MESSAGE: usize = 1024;
const USB_PACKET: usize = 64;
/// How long the device is off the bus while it resets and re-enumerates: on real USB at least the host's 100 ms
/// connect debounce plus the reset and enumeration.
const REENUMERATE: Duration = Duration::from_millis(200);

/// A firmware image of the simulated app: a header with magic, version, length and checksum, then the body. The
/// simulated boot ROM boots it only when all of it is intact.
pub fn app_image(version: u32, len: usize) -> Vec<u8> {
    let body: Vec<u8> = (0..len - 16)
        .map(|i| (i as u32).wrapping_mul(31).wrapping_add(version) as u8)
        .collect();
    let sum = body.iter().fold(0u32, |s, b| s.wrapping_add(*b as u32));
    let mut image = b"WWFW".to_vec();
    image.extend(version.to_le_bytes());
    image.extend((len as u32).to_le_bytes());
    image.extend(sum.to_le_bytes());
    image.extend(body);
    image
}

/// The version of the app in flash, `None` when there is no intact image.
fn bootable(sim: &SimRp2350) -> Option<u32> {
    let header = sim.flash(FLASH_BASE, 16);
    let word = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    if &header[..4] != b"WWFW" {
        return None;
    }
    let len = word(8) as usize;
    let body = sim.flash(FLASH_BASE + 16, len.checked_sub(16)?);
    let sum = body.iter().fold(0u32, |s, b| s.wrapping_add(*b as u32));
    (sum == word(12)).then_some(word(4))
}

/// What the device shows on USB.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Resetting, not enumerated.
    Off,
    /// The app with this image version.
    App(u32),
    /// The boot ROM in BOOTSEL mode.
    Bootsel,
}

/// A running simulated device, stopped when dropped.
pub struct Report {
    pub serial: String,
    pub sim: SimRp2350,
    mode: Arc<Mutex<Mode>>,
    boots: mpsc::UnboundedReceiver<Mode>,
    _stop: mpsc::UnboundedSender<()>,
}

impl Report {
    /// Start a device with chip id `chip_id` and `flash` at the start of flash.
    pub fn start(chip_id: u64, flash: &[u8]) -> Report {
        let sim = SimRp2350::new(chip_id, 256 * 1024);
        sim.preload(FLASH_BASE, flash);
        let serial = sim.serial();
        let mode = Arc::new(Mutex::new(Mode::Off));
        let (boots_tx, boots) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = mpsc::unbounded_channel();
        let (t_sim, t_mode, t_serial) = (sim.clone(), mode.clone(), serial.clone());
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(device_thread(t_serial, t_sim, t_mode, boots_tx, stop_rx))
        });
        Report {
            serial,
            sim,
            mode,
            boots,
            _stop: stop_tx,
        }
    }

    /// The bus this device is on.
    pub fn bus(&self) -> SimBus {
        SimBus {
            serial: self.serial.clone(),
            mode: self.mode.clone(),
            sim: self.sim.clone(),
        }
    }

    /// Wait for the next boot, returns what runs.
    pub async fn booted(&mut self) -> Mode {
        tokio::time::timeout(Duration::from_secs(5), self.boots.recv())
            .await
            .expect("device boots")
            .expect("device thread runs")
    }
}

async fn device_thread(
    serial: String,
    sim: SimRp2350,
    mode: Arc<Mutex<Mode>>,
    boots: mpsc::UnboundedSender<Mode>,
    mut stop: mpsc::UnboundedReceiver<()>,
) {
    let mut to_bootsel = false;
    loop {
        tokio::time::sleep(REENUMERATE).await;
        // no intact image: the boot ROM stays in BOOTSEL, as a blank RP2350 does
        let image = if to_bootsel { None } else { bootable(&sim) };
        let into = match image {
            Some(version) => {
                *mode.lock().unwrap() = Mode::App(version);
                _ = boots.send(Mode::App(version));
                let app = App {
                    version,
                    reboot: None,
                };
                let Some(into) = run_app(app, &serial, &mut stop).await else {
                    return;
                };
                *mode.lock().unwrap() = Mode::Off;
                into
            }
            None => BootTarget::Updater,
        };
        if into == BootTarget::App {
            continue;
        }
        tokio::time::sleep(REENUMERATE).await;
        sim.reset();
        *mode.lock().unwrap() = Mode::Bootsel;
        _ = boots.send(Mode::Bootsel);
        let reboot = loop {
            if let Some(r) = sim.reboot_request() {
                break r;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(5)) => {}
                _ = stop.recv() => return,
            }
        };
        // the reply to REBOOT2 is out, the ROM waits the delay and resets
        tokio::time::sleep(reboot.delay).await;
        *mode.lock().unwrap() = Mode::Off;
        to_bootsel = reboot.flags & REBOOT2_TYPE_MASK == REBOOT2_TYPE_BOOTSEL;
    }
}

/// The app: `UpdateNode` and `BootControl` of an in-place RP2350 device whose updater is the boot ROM.
pub struct App {
    version: u32,
    reboot: Option<BootTarget>,
}

impl App {
    fn node_node(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<NodeInfo<'_>> {
        Ready(NodeInfo {
            role: Role::App,
            profile: Profile::InPlace,
            product: MaxLen::new("sim-rp2350").unwrap(),
            geometry_id: 0,
            app_version: Some(self.version),
            version_floor: 0,
            manifest_digest: None,
            boot_state: BootState::Confirmed,
            last_result: LastResult {
                outcome: Outcome::Nothing,
                version: 0,
            },
            offers: Offers {
                boot_control: true,
                inventory: false,
                stage: false,
                stage_bulk: false,
            },
        })
    }

    fn boot_boot_state(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<BootState> {
        Ready(BootState::Confirmed)
    }

    fn boot_confirm(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<Result<(), Error>> {
        Ready(Ok(()))
    }

    fn boot_revert(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<Result<(), Error>> {
        Ready(Err(Error::Unsupported))
    }

    fn boot_reboot(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        into: BootTarget,
    ) -> RpcResult<Result<(), Error>> {
        self.reboot = Some(into);
        Ready(Ok(()))
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        dfu_lean_app_api :: DfuLeanApp for super::App,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
    );
}

struct Backend(App);

impl WireWeaverAsyncApiBackend for Backend {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        _medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        out.queued(|queue| self.0.process_request_bytes(data, scratch, queue, ()))
            .await
    }

    fn version(&self) -> ww_device::ww_link::FullVersion<'_> {
        dfu_lean_app_api::DFU_LEAN_APP_FULL_GID
    }
}

/// Run the app until it is asked to reboot, `None` when stopped.
async fn run_app(
    app: App,
    serial: &str,
    stop: &mut mpsc::UnboundedReceiver<()>,
) -> Option<BootTarget> {
    let mut tx_buf = vec![0u8; USB_PACKET];
    let mut rx_buf = vec![0u8; MAX_MESSAGE + USB_PACKET];
    let mut scratch = vec![0u8; MAX_MESSAGE];
    let mut event_scratch = vec![0u8; MAX_MESSAGE];
    let (sink, source) = in_process::usb_device(serial, USB_PACKET);
    let tx = ww_device::FramedTx::new(sink, &mut tx_buf);
    let rx = ww_device::FramedRx::new(source, &mut rx_buf);
    let config = LinkConfig::new(
        dfu_lean_app_api::DFU_LEAN_APP_FULL_GID,
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
    let mut backend = Backend(app);
    loop {
        tokio::select! {
            ready = server.wait() => {
                _ = server.handle(ready, &mut backend).await;
            }
            _ = stop.recv() => return None,
        }
        if let Some(into) = backend.0.reboot.take() {
            // the reply is out with the Disconnect, then the device resets
            server.disconnect(DisconnectReason::RequestByUser).await;
            return Some(into);
        }
    }
}

/// The USB bus as the host sees it, with this one device on it.
#[derive(Clone)]
pub struct SimBus {
    serial: String,
    mode: Arc<Mutex<Mode>>,
    sim: SimRp2350,
}

impl SimBus {
    /// Open the PICOBOOT interface of the BOOTSEL device `info`.
    pub fn open(&self, info: &DeviceInfo) -> Result<SimRp2350, ww_picoboot::Error> {
        let bootsel = *self.mode.lock().unwrap() == Mode::Bootsel;
        if bootsel && info.serials.contains(&self.serial) {
            Ok(self.sim.clone())
        } else {
            Err(ww_picoboot::Error::Other(format!("{info} is gone")))
        }
    }
}

impl Bus for SimBus {
    async fn list(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        let usb = |vid, pid| {
            Some(UsbLocation {
                bus_id: "1".into(),
                port_chain: vec![2],
                vid,
                pid,
            })
        };
        let mode = *self.mode.lock().unwrap();
        Ok(match mode {
            Mode::Off => vec![],
            Mode::App(_) => vec![DeviceInfo {
                location: format!("sim {APP_VID:04x}:{APP_PID:04x}"),
                manufacturer: "vhrd.tech".into(),
                product: "sim rePort".into(),
                serials: vec![self.serial.clone()],
                user_label: String::new(),
                api: Some(ApiInfo {
                    gid: "dfu_lean_app_api".into(),
                    version: semver::Version::new(0, 1, 0),
                    signature: wire_weaver_client::ww_version::ApiHashOwned { hash: vec![] },
                }),
                usb: usb(APP_VID, APP_PID),
            }],
            Mode::Bootsel => vec![DeviceInfo {
                location: format!("sim {RPI_VID:04x}:{RP2350_BOOTSEL_PID:04x}"),
                manufacturer: "Raspberry Pi".into(),
                product: "RP2350 Boot".into(),
                serials: vec![self.serial.clone()],
                user_label: String::new(),
                api: None,
                usb: usb(RPI_VID, RP2350_BOOTSEL_PID),
            }],
        })
    }
}

#[cfg(test)]
mod tests;
