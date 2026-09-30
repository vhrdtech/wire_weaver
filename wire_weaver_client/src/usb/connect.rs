use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::DeviceHandle;
use crate::event_loop::command::Command;
use anyhow::{Context, anyhow, bail};
use nusb::descriptors::TransferType;
use nusb::{Device, DeviceInfo, Interface, MaybeFuture};
use tokio::sync::mpsc;
use tracing::trace;

pub(crate) struct ConnectOk {
    pub(crate) device: Device,
    pub(crate) interface: Interface,
    pub(crate) transfer_type: TransferType,
    pub(crate) max_packet_size: usize,
}

pub(crate) fn connect(di: &DeviceInfo) -> anyhow::Result<ConnectOk> {
    trace!("connecting to USB device: {di:?}");
    let device = di.open().wait().map_err(with_hint)?;
    let device_clone = device.clone();
    let active_configuration = device_clone
        .active_configuration()
        .context("getting active configuration")?;
    let alt = active_configuration
        .interface_alt_settings()
        .next()
        .ok_or(anyhow!("No interfaces found in active USB configuration"))?;
    let ep = alt
        .endpoints()
        .next()
        .ok_or(anyhow!("No endpoints found in active USB configuration"))?;
    let interface = device.claim_interface(0).wait().map_err(with_hint)?;
    Ok(ConnectOk {
        device,
        interface,
        transfer_type: ep.transfer_type(),
        max_packet_size: ep.max_packet_size(),
    })
}

// async fn wait_device(filter: &DeviceFilter, timeout: OnError) -> Result<DeviceInfo, Error> {
//     let mut watch = nusb::watch_devices().map_err(|e| Error::Transport(format!("{}", e)))?;
//     let devices = nusb::list_devices()
//         .await
//         .map_err(|e| Error::Transport(format!("{}", e)))?;
//     for d in devices {
//         if filter.matches_nusb(&d)? {
//             return Ok(d);
//         }
//     }
//     trace!("waiting for USB device to connect...");
//     match timeout {
//         OnError::ExitImmediately => Err(Error::DeviceNotFound),
//         OnError::RetryFor { mut timeout, .. } => loop {
//             let wait_started = Instant::now();
//             tokio::select! {
//                 _ = tokio::time::sleep(timeout) => {
//                     return Err(Error::DeviceNotFound)
//                 }
//                 hotplug_event = watch.next() => {
//                     let Some(hotplug_event) = hotplug_event else {
//                         return Err(Error::Transport(UsbError::WatcherReturnedNone.into()))
//                     };
//                     if let HotplugEvent::Connected(di) = hotplug_event {
//                         if filter.matches_nusb(&di)? {
//                             // as per nusb docs, must wait a bit on Windows after getting watched device, otherwise connection fails
//                             #[cfg(target_os = "windows")]
//                             tokio::time::sleep(std::time::Duration::from_millis(10)).await; // TODO: is 10ms enough on slow Windows VM?

//                             return Ok(di)
//                         }
//                     }

//                     let now = Instant::now();
//                     let dt = now.duration_since(wait_started);
//                     if dt > timeout {
//                         return Err(Error::DeviceNotFound)
//                     } else {
//                         timeout -= dt;
//                     }
//                 }
//             }
//         },
//         OnError::KeepRetrying => {
//             while let Some(hotplug_event) = watch.next().await {
//                 if let HotplugEvent::Connected(di) = hotplug_event {
//                     if filter.matches_nusb(&di)? {
//                         return Ok(di);
//                     }
//                 }
//             }
//             Err(Error::Transport(UsbError::WatcherReturnedNone.into()))
//         }
//     }
// }

/// Explain the likely cause of the most common errors
fn with_hint(e: nusb::Error) -> anyhow::Error {
    let hint = match e.kind() {
        nusb::ErrorKind::Busy => {
            "device is already open by another program or another connection in this one"
        }
        nusb::ErrorKind::PermissionDenied if cfg!(target_os = "linux") => {
            "no access to the device, run 'ww udev --help' to see how to install a udev rule"
        }
        _ => return e.into(),
    };
    anyhow!("{e}; {hint}")
}

pub(crate) async fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let devices = nusb::list_devices()
        .await
        .context("listing USB devices")?;
    start_event_loop(c, devices, cmd_rx)
}

pub(crate) fn try_connect_blocking(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    // TODO: figure out if nusb::list_devices() hangs in other scenarios, apart from enumeration problems on Linux, add timeout
    let devices = nusb::list_devices()
        .wait()
        .context("listing USB devices")?;
    start_event_loop(c, devices, cmd_rx)
}

pub(crate) enum Selected {
    /// Event loop is started, connect command must be sent to it with this handle
    Device {
        handle: DeviceHandle,
        info: Box<crate::DeviceInfo>,
    },
    /// No device matched, event loop is not started
    NotFound {
        /// WireWeaver devices that did not pass the filters
        unmatched: Vec<crate::DeviceInfo>,
    },
}

fn start_event_loop(
    c: &ValidatedConfig,
    devices: impl Iterator<Item = nusb::DeviceInfo>,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let (nusb_info, info) = match select_matching(c, devices)? {
        Ok(matched) => matched,
        Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(async move {
        super::event_loop::usb_worker(cmd_rx).await;
    });
    Ok(Selected::Device {
        handle: Box::new(nusb_info),
        info: Box::new(info),
    })
}

/// Returns the only matching device, or WireWeaver devices that did not match if none did.
#[allow(clippy::type_complexity)]
fn select_matching(
    c: &ValidatedConfig,
    devices: impl Iterator<Item = nusb::DeviceInfo>,
) -> Result<Result<(nusb::DeviceInfo, crate::DeviceInfo), Vec<crate::DeviceInfo>>, crate::Error> {
    // without an explicit VID:PID or path, only consider devices reporting WireWeaver API id
    let by_location = c.pieces.iter().any(|p| {
        matches!(
            p,
            ConfigPiece::UsbVidPid { .. } | ConfigPiece::UsbPath { .. }
        )
    });
    let mut matching = vec![];
    let mut unmatched = vec![];
    for nusb_info in devices.into_iter() {
        let info = crate::DeviceInfo::from(&nusb_info);
        if (by_location || info.api.is_some()) && info.is_matching(&c.pieces) {
            matching.push((nusb_info, info));
        } else if info.api.is_some() {
            unmatched.push(info);
        }
    }
    if let Some(matched) = matching.pop() {
        if matching.is_empty() {
            Ok(Ok(matched))
        } else {
            let mut devices = vec![matched.1];
            devices.extend(matching.drain(..).map(|(_, info)| info));
            Err(crate::Error::AmbiguousDeviceChoice(devices))
        }
    } else {
        Ok(Err(unmatched))
    }
}

/// List connected USB devices that report WireWeaver API id, without opening them.
pub async fn list_devices() -> Result<Vec<crate::DeviceInfo>, anyhow::Error> {
    let mut devices = list_all_devices().await?;
    devices.retain(|d| d.api.is_some());
    Ok(devices)
}

/// List all connected USB devices, including the ones not reporting WireWeaver API id, without opening them.
pub async fn list_all_devices() -> Result<Vec<crate::DeviceInfo>, anyhow::Error> {
    Ok(nusb::list_devices()
        .await?
        .map(|d| crate::DeviceInfo::from(&d))
        .collect())
}
