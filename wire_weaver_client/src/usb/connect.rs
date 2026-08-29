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
    let device = di.open().wait()?;
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
    let interface = device.claim_interface(0).wait()?;
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

pub(crate) async fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Option<DeviceHandle>, anyhow::Error> {
    let devices = nusb::list_devices().await?;
    start_event_loop(c, devices, cmd_rx)
}

pub(crate) fn try_connect_blocking(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Option<DeviceHandle>, anyhow::Error> {
    // TODO: figure out if nusb::list_devices() hangs in other scenarios, apart from enumeration problems on Linux, add timeout
    let devices = nusb::list_devices().wait()?;
    start_event_loop(c, devices, cmd_rx)
}

fn start_event_loop(
    c: &ValidatedConfig,
    devices: impl Iterator<Item = nusb::DeviceInfo>,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Option<DeviceHandle>, anyhow::Error> {
    let Some(matched) = select_matching(c, devices)? else {
        return Ok(None);
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(async move {
        super::event_loop::usb_worker(cmd_rx).await;
    });
    let handle = Box::new(matched);
    Ok(Some(handle))
}

fn select_matching(
    c: &ValidatedConfig,
    devices: impl Iterator<Item = nusb::DeviceInfo>,
) -> Result<Option<nusb::DeviceInfo>, crate::Error> {
    let mut matching = vec![];
    for nusb_info in devices.into_iter() {
        let info = crate::DeviceInfo::from(&nusb_info);
        let vid_pid_match = vid_pid_match(&c.pieces, &nusb_info);
        let chain_match = port_chain_match(&c.pieces, &nusb_info);
        if vid_pid_match || chain_match || info.is_matching(c) {
            matching.push((nusb_info, info));
        }
    }
    if let Some((nusb_info, info)) = matching.pop() {
        if matching.is_empty() {
            Ok(Some(nusb_info))
        } else {
            let mut devices = vec![info];
            devices.extend(matching.drain(..).map(|(_, info)| info));
            Err(crate::Error::AmbiguousDeviceChoice(devices))
        }
    } else {
        Ok(None)
    }
}

fn vid_pid_match(pieces: &[ConfigPiece], info: &nusb::DeviceInfo) -> bool {
    pieces.iter().any(|p| {
        if let ConfigPiece::UsbVidPid { vid, pid } = p {
            info.vendor_id() == *vid && info.product_id() == *pid
        } else {
            false
        }
    })
}

fn port_chain_match(pieces: &[ConfigPiece], info: &nusb::DeviceInfo) -> bool {
    pieces.iter().any(|p| {
        if let ConfigPiece::UsbPath { bus_id, port_chain } = p {
            info.bus_id() == bus_id && info.port_chain() == port_chain
        } else {
            false
        }
    })
}
