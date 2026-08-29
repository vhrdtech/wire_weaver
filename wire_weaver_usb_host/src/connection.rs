use anyhow::{Context, Result, anyhow};
use nusb::descriptors::TransferType;
use nusb::{Device, DeviceInfo, Interface, MaybeFuture};
use tracing::trace;

pub(crate) struct ConnectOk {
    pub(crate) device: Device,
    pub(crate) interface: Interface,
    pub(crate) transfer_type: TransferType,
    pub(crate) max_packet_size: usize,
}

pub(crate) fn connect(di: &DeviceInfo) -> Result<ConnectOk> {
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
