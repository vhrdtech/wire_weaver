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
use ww_stm32_dfu::proto::{ROM_DFU_PID, ST_VID};

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
    /// An STM32 system bootloader in DFU mode (`0483:df11`), ready for DfuSe.
    Stm32Dfu(DeviceInfo),
    /// Something else.
    Other(DeviceInfo),
}

impl Found {
    pub fn classify(info: DeviceInfo) -> Found {
        if is_rp_bootsel(&info) {
            Found::RpBootsel(info)
        } else if is_stm32_dfu(&info) {
            Found::Stm32Dfu(info)
        } else if info.api.is_some() {
            Found::WireWeaver(info)
        } else {
            Found::Other(info)
        }
    }

    pub fn info(&self) -> &DeviceInfo {
        match self {
            Found::WireWeaver(i) | Found::RpBootsel(i) | Found::Stm32Dfu(i) | Found::Other(i) => i,
        }
    }

    pub fn into_info(self) -> DeviceInfo {
        match self {
            Found::WireWeaver(i) | Found::RpBootsel(i) | Found::Stm32Dfu(i) | Found::Other(i) => i,
        }
    }
}

/// Whether `info` is an RP2040 or RP2350 boot ROM in BOOTSEL mode.
pub fn is_rp_bootsel(info: &DeviceInfo) -> bool {
    info.usb.as_ref().is_some_and(|u| {
        u.vid == RPI_VID && (u.pid == RP2040_BOOTSEL_PID || u.pid == RP2350_BOOTSEL_PID)
    })
}

/// Whether `info` is an STM32 system bootloader in DFU mode.
pub fn is_stm32_dfu(info: &DeviceInfo) -> bool {
    info.usb
        .as_ref()
        .is_some_and(|u| u.vid == ST_VID && u.pid == ROM_DFU_PID)
}

/// The serial number the STM32 system bootloader reports for a chip with unique id words `uid`, see
/// [ww_stm32_dfu::rom_serial] (not yet checked on an H72x). A firmware that reports the same serial number can be
/// followed into the bootloader and back.
pub fn stm32_serial(uid: [u32; 3]) -> String {
    ww_stm32_dfu::rom_serial(uid)
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
    #[error("{serial} in the STM32 DFU bootloader: {source}")]
    Stm32Dfu {
        serial: String,
        source: ww_stm32_dfu::Error,
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

    /// Wait for the STM32 system bootloader in DFU mode.
    pub async fn back_in_stm32_dfu(
        &self,
        bus: &mut impl Bus,
        timeout: Duration,
    ) -> Result<DeviceInfo, FollowError> {
        self.back(bus, timeout, "no STM32 DFU device", |f| {
            matches!(f, Found::Stm32Dfu(_))
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

/// How long each step of [rp2_update_with] and [stm32_dfu_update_with] may take.
#[derive(Clone, Debug)]
pub struct Timeouts {
    /// From the reboot request until the boot ROM enumerates (BOOTSEL, STM32 DFU).
    pub to_bootsel: Duration,
    /// From the reboot out of BOOTSEL until the firmware enumerates.
    pub to_app: Duration,
    /// How long opening the DFU interface is retried after the bootloader showed up in the listing: it can be
    /// listed before the host's USB stack lets it be opened (permissions, a driver still binding).
    pub open: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            to_bootsel: Duration::from_secs(10),
            to_app: Duration::from_secs(10),
            open: Duration::from_secs(3),
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

/// [stm32_dfu_update_with] on real USB devices: write `image`, then jump to `leave_addr` (the start of the first
/// memory region, flash, when `None`).
#[cfg(feature = "stm32_dfu")]
pub async fn stm32_dfu_update(
    serial: &str,
    image: &ww_stm32_dfu::Image,
    leave_addr: Option<u32>,
    progress: &mut impl ww_stm32_dfu::Progress,
) -> Result<DeviceInfo, FollowError> {
    stm32_dfu_update_with(
        &Follow::new(serial),
        &mut UsbBus,
        async |info: &DeviceInfo| {
            let serial = info.serials.first().cloned().unwrap_or_default();
            ww_stm32_dfu::usb::NusbIo::open_by_serial(&serial, 0).await
        },
        image,
        leave_addr,
        progress,
        Timeouts::default(),
    )
    .await
}

/// The second half of an update in the STM32 system bootloader (ROM DFU, AN2606), after the firmware was asked to
/// reboot into it: wait for the bootloader with the same serial number, `open` its DFU interface, erase and write
/// `image` and verify it, leave DFU by jumping to `leave_addr` (the start of the first memory region when `None`),
/// wait for the firmware to come back. Returns the device as it came back, connect to it by its serial number.
///
/// `open` is called again for [Timeouts::open] when it fails: the bootloader can be listed before it can be opened.
#[allow(clippy::too_many_arguments)]
pub async fn stm32_dfu_update_with<I: ww_stm32_dfu::DfuIo>(
    follow: &Follow,
    bus: &mut impl Bus,
    open: impl AsyncFn(&DeviceInfo) -> Result<I, ww_stm32_dfu::Error>,
    image: &ww_stm32_dfu::Image,
    leave_addr: Option<u32>,
    progress: &mut impl ww_stm32_dfu::Progress,
    timeouts: Timeouts,
) -> Result<DeviceInfo, FollowError> {
    let dfu_err = |source| FollowError::Stm32Dfu {
        serial: follow.serial.clone(),
        source,
    };
    let rom = follow.back_in_stm32_dfu(bus, timeouts.to_bootsel).await?;
    tracing::debug!("{} is in the STM32 DFU bootloader: {rom}", follow.serial);
    let io = open_retrying(async || open(&rom).await, timeouts.open)
        .await
        .map_err(dfu_err)?;
    let mut dfu = ww_stm32_dfu::Dfuse::new(io).map_err(dfu_err)?;
    dfu.flash(image, &ww_stm32_dfu::FlashOptions::default(), progress)
        .await
        .map_err(dfu_err)?;
    let leave_addr = leave_addr.unwrap_or_else(|| dfu.layout().regions[0].start);
    dfu.leave(leave_addr).await.map_err(dfu_err)?;
    drop(dfu);
    follow.back_in_wire_weaver(bus, timeouts.to_app).await
}

/// `open`, tried again every 100 ms until it works or `within` is over; the last error otherwise. A device just
/// listed can refuse to open for a moment (udev still setting its permissions, a driver binding).
pub async fn open_retrying<I>(
    open: impl AsyncFn() -> Result<I, ww_stm32_dfu::Error>,
    within: Duration,
) -> Result<I, ww_stm32_dfu::Error> {
    let deadline = Instant::now() + within;
    loop {
        match open().await {
            Ok(io) => return Ok(io),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(e) => {
                tracing::debug!("opening the DFU device: {e}, trying again");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}
