//! Follow a device across a reboot into other firmware (CLT-21): the same unit comes back under the same serial
//! number, but maybe as another USB device (an updater with its own API, or a boot ROM that doesn't speak WireWeaver
//! at all), after it re-enumerated.
//!
//! The sequence for an RP2350 that updates in the ROM bootloader (`ww_dfu::BootControl::reboot(BootTarget::Updater)`
//! on a firmware whose USB serial number is the chip id, in the format the boot ROM uses):
//! ```ignore
//! let serial = device_info.serials[0].clone(); // before rebooting
//! app.boot().reboot(BootTarget::Updater).call().await?;
//! drop(app);
//! let back = follow::rp2_update(&serial, &image, &mut ()).await?; // finds BOOTSEL, flashes, reboots
//! let app = App::config(|c| c.usb().serial_eq(serial)).connect().await?;
//! ```
//!
//! [Follow] does the waiting on any [Bus]: [UsbBus] for real devices, a simulated one in tests.

use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::DeviceInfo;

use ww_picoboot::proto::{RP2040_BOOTSEL_PID, RP2350_BOOTSEL_PID, RPI_VID};
use ww_picoboot::{Image, PicobootIo, Progress};

/// Where devices are looked for.
pub trait Bus {
    /// Devices present now.
    fn list(&mut self) -> impl Future<Output = anyhow::Result<Vec<DeviceInfo>>> + Send;
}

/// Real USB devices, through nusb.
#[cfg(feature = "usb")]
pub struct UsbBus;

#[cfg(feature = "usb")]
impl Bus for UsbBus {
    async fn list(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        crate::usb::list_all_devices().await
    }
}

/// What a device with the followed serial number turned out to be.
#[derive(Debug)]
pub enum Found {
    /// A WireWeaver device, with its API id if it reports one.
    WireWeaver(DeviceInfo),
    /// A Raspberry Pi RP2040 / RP2350 boot ROM in BOOTSEL mode, ready for PICOBOOT.
    RpBootsel(DeviceInfo),
    /// Something else.
    Other(DeviceInfo),
}

impl Found {
    pub fn classify(info: DeviceInfo) -> Found {
        if is_rp_bootsel(&info) {
            Found::RpBootsel(info)
        } else if info.api.is_some() {
            Found::WireWeaver(info)
        } else {
            Found::Other(info)
        }
    }

    pub fn info(&self) -> &DeviceInfo {
        match self {
            Found::WireWeaver(i) | Found::RpBootsel(i) | Found::Other(i) => i,
        }
    }

    pub fn into_info(self) -> DeviceInfo {
        match self {
            Found::WireWeaver(i) | Found::RpBootsel(i) | Found::Other(i) => i,
        }
    }
}

/// Whether `info` is an RP2040 or RP2350 boot ROM in BOOTSEL mode.
pub fn is_rp_bootsel(info: &DeviceInfo) -> bool {
    info.usb.as_ref().is_some_and(|u| {
        u.vid == RPI_VID && (u.pid == RP2040_BOOTSEL_PID || u.pid == RP2350_BOOTSEL_PID)
    })
}

/// The serial number the RP2350 boot ROM reports in BOOTSEL mode for `chip_id`: 16 upper case hex digits. A firmware
/// that reports the same serial number can be followed into BOOTSEL and back.
pub fn rp2350_serial(chip_id: u64) -> String {
    format!("{chip_id:016X}")
}

#[derive(Debug, thiserror::Error)]
pub enum FollowError {
    #[error("{serial}: {what} within {waited:?}{}", describe_seen(.seen))]
    Timeout {
        serial: String,
        what: &'static str,
        waited: Duration,
        /// What had the serial number when the time was up.
        seen: Vec<String>,
    },
    #[error("listing devices: {0:#}")]
    Bus(anyhow::Error),
    #[error("{serial} in BOOTSEL: {source}")]
    Picoboot {
        serial: String,
        source: ww_picoboot::Error,
    },
}

fn describe_seen(seen: &[String]) -> String {
    if seen.is_empty() {
        String::new()
    } else {
        format!(", present: {}", seen.join("; "))
    }
}

/// Waits for the device with a serial number to go away and come back.
#[derive(Clone, Debug)]
pub struct Follow {
    /// Compared ignoring case, as the other device filters do.
    pub serial: String,
    /// How often the bus is listed.
    pub poll: Duration,
}

impl Follow {
    pub fn new(serial: impl Into<String>) -> Self {
        Follow {
            serial: serial.into(),
            poll: Duration::from_millis(50),
        }
    }

    fn with_serial(&self, devices: Vec<DeviceInfo>) -> Vec<DeviceInfo> {
        devices
            .into_iter()
            .filter(|d| {
                d.serials
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&self.serial))
            })
            .collect()
    }

    /// Wait until no device has the serial number: the device reset. Not needed before [Follow::back] when what
    /// comes back can be told apart from what went away (an app rebooting into BOOTSEL), needed when it can't
    /// (an app rebooting into itself). The device has to be off the bus for longer than [Follow::poll] to be seen
    /// gone; USB re-enumeration takes well over 100 ms (the host's connect debounce alone).
    pub async fn gone(&self, bus: &mut impl Bus, timeout: Duration) -> Result<(), FollowError> {
        let deadline = Instant::now() + timeout;
        loop {
            let present = self.with_serial(bus.list().await.map_err(FollowError::Bus)?);
            if present.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(self.timeout("did not go away", timeout, present));
            }
            tokio::time::sleep(self.poll).await;
        }
    }

    /// Wait for a device with the serial number that `want` accepts.
    pub async fn back(
        &self,
        bus: &mut impl Bus,
        timeout: Duration,
        what: &'static str,
        want: impl Fn(&Found) -> bool,
    ) -> Result<Found, FollowError> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut present = vec![];
            for info in self.with_serial(bus.list().await.map_err(FollowError::Bus)?) {
                let found = Found::classify(info);
                if want(&found) {
                    return Ok(found);
                }
                present.push(found.into_info());
            }
            if Instant::now() >= deadline {
                return Err(self.timeout(what, timeout, present));
            }
            tokio::time::sleep(self.poll).await;
        }
    }

    /// Wait for the RP2040 / RP2350 boot ROM in BOOTSEL mode.
    pub async fn back_in_bootsel(
        &self,
        bus: &mut impl Bus,
        timeout: Duration,
    ) -> Result<DeviceInfo, FollowError> {
        self.back(bus, timeout, "no BOOTSEL device", |f| {
            matches!(f, Found::RpBootsel(_))
        })
        .await
        .map(Found::into_info)
    }

    /// Wait for a WireWeaver device, whatever API it has.
    pub async fn back_in_wire_weaver(
        &self,
        bus: &mut impl Bus,
        timeout: Duration,
    ) -> Result<DeviceInfo, FollowError> {
        self.back(bus, timeout, "no WireWeaver device", |f| {
            matches!(f, Found::WireWeaver(_))
        })
        .await
        .map(Found::into_info)
    }

    fn timeout(&self, what: &'static str, waited: Duration, seen: Vec<DeviceInfo>) -> FollowError {
        FollowError::Timeout {
            serial: self.serial.clone(),
            what,
            waited,
            seen: seen.iter().map(|d| d.to_string()).collect(),
        }
    }
}

/// How long each step of [rp2_update_with] may take.
#[derive(Clone, Debug)]
pub struct Timeouts {
    /// From the reboot request until the boot ROM enumerates.
    pub to_bootsel: Duration,
    /// From the reboot out of BOOTSEL until the firmware enumerates.
    pub to_app: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            to_bootsel: Duration::from_secs(10),
            to_app: Duration::from_secs(10),
        }
    }
}

/// [rp2_update_with] on real USB devices.
#[cfg(feature = "picoboot")]
pub async fn rp2_update(
    serial: &str,
    image: &Image,
    progress: &mut impl Progress,
) -> Result<DeviceInfo, FollowError> {
    rp2_update_with(
        &Follow::new(serial),
        &mut UsbBus,
        async |info: &DeviceInfo| {
            let serial = info.serials.first().cloned().unwrap_or_default();
            ww_picoboot::usb::NusbIo::open_by_serial(&serial).await
        },
        image,
        progress,
        Timeouts::default(),
    )
    .await
}

/// The second half of an update in the RP2040 / RP2350 boot ROM, after the firmware was asked to reboot into BOOTSEL:
/// wait for the boot ROM with the same serial number, `open` its PICOBOOT interface, write `image` into flash and
/// verify it, reboot, wait for the firmware to come back. Returns the device as it came back, connect to it by its
/// serial number.
pub async fn rp2_update_with<I: PicobootIo>(
    follow: &Follow,
    bus: &mut impl Bus,
    open: impl AsyncFnOnce(&DeviceInfo) -> Result<I, ww_picoboot::Error>,
    image: &Image,
    progress: &mut impl Progress,
    timeouts: Timeouts,
) -> Result<DeviceInfo, FollowError> {
    let picoboot_err = |source| FollowError::Picoboot {
        serial: follow.serial.clone(),
        source,
    };
    let bootsel = follow.back_in_bootsel(bus, timeouts.to_bootsel).await?;
    tracing::debug!("{} is in BOOTSEL: {bootsel}", follow.serial);
    let io = open(&bootsel).await.map_err(picoboot_err)?;
    let mut picoboot = ww_picoboot::Picoboot::new(io).await.map_err(picoboot_err)?;
    picoboot
        .flash(image, progress)
        .await
        .map_err(picoboot_err)?;
    // as `picotool load -x`: boot the image just written
    let base = image.segments.first().map(|s| s.addr).unwrap_or_default();
    picoboot
        .reboot2(
            ww_picoboot::proto::Reboot2::FlashUpdate { base },
            ww_picoboot::proto::REBOOT_DELAY,
        )
        .await
        .map_err(picoboot_err)?;
    drop(picoboot);
    follow.back_in_wire_weaver(bus, timeouts.to_app).await
}
