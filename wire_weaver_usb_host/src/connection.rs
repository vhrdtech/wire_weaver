use crate::UsbError;
use futures_lite::StreamExt;
use nusb::descriptors::TransferType;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceInfo, Interface};
use std::time::Instant;
use tracing::{error, trace};
use wire_weaver_client_common::{DeviceFilter, Error, OnError};

pub(crate) async fn connect(
    filter: DeviceFilter,
    mut timeout: OnError,
) -> Result<(Interface, DeviceInfo, TransferType, usize), Error> {
    let wait_started = Instant::now();
    loop {
        // TODO: figure out if nusb::list_devices() hangs in other scenarios, apart from enumeration problems on Linux
        let devices = nusb::list_devices()
            .await
            .map_err(|e| Error::Transport(format!("{}", e)))?;
        let mut di = None;
        for d in devices {
            if filter.matches_nusb(&d)? {
                di = Some(d);
                break;
            }
        }
        let di = match di {
            Some(di) => di,
            None => {
                if timeout == OnError::ExitImmediately {
                    return Err(Error::DeviceNotFound);
                } else {
                    wait_device(&filter, timeout).await?
                }
            }
        };
        trace!("connecting to USB device: {di:?}");
        let (interface, transfer_type, max_packet_size) =
            {
                let dev = di
                    .open()
                    .await
                    .map_err(|e| Error::Transport(format!("{}", e)))?;
                let active_configuration = dev
                    .active_configuration()
                    .map_err(|e| Error::Transport(format!("{}", e)))?;
                let alt = active_configuration.interface_alt_settings().next().ok_or(
                    Error::Transport("No interfaces found in active USB configuration".into()),
                )?;
                let ep = alt.endpoints().next().ok_or(Error::Transport(
                    "No endpoints found in active USB configuration".into(),
                ))?;
                (
                    dev.claim_interface(0).await,
                    ep.transfer_type(),
                    ep.max_packet_size(),
                )
            };
        match interface {
            Ok(interface) => {
                return Ok((interface, di, transfer_type, max_packet_size));
            }
            Err(e) => match &mut timeout {
                OnError::ExitImmediately => {
                    error!("device {di:?} open failed: {e:?}, no timeout, bailing");
                    return Err(Error::DeviceNotFound);
                }
                OnError::RetryFor { timeout, .. } => {
                    let now = Instant::now();
                    let dt = now.duration_since(wait_started);
                    if dt > *timeout {
                        error!("device {di:?} open failed: {e:?}, timeout expired, bailing");
                        return Err(Error::DeviceNotFound);
                    } else {
                        *timeout -= dt;
                        error!(
                            "device {di:?} open failed: {e:?}, waiting another device for {}s...",
                            timeout.as_secs(),
                        );
                    }
                }
                OnError::KeepRetrying => {
                    error!("device {di:?} open failed: {e:?}, waiting for another...");
                }
            },
        }
    }
}

async fn wait_device(filter: &DeviceFilter, timeout: OnError) -> Result<DeviceInfo, Error> {
    let mut watch = nusb::watch_devices().map_err(|e| Error::Transport(format!("{}", e)))?;
    let devices = nusb::list_devices()
        .await
        .map_err(|e| Error::Transport(format!("{}", e)))?;
    for d in devices {
        if filter.matches_nusb(&d)? {
            return Ok(d);
        }
    }
    trace!("waiting for USB device to connect...");
    match timeout {
        OnError::ExitImmediately => Err(Error::DeviceNotFound),
        OnError::RetryFor { mut timeout, .. } => loop {
            let wait_started = Instant::now();
            tokio::select! {
                _ = tokio::time::sleep(timeout) => {
                    return Err(Error::DeviceNotFound)
                }
                hotplug_event = watch.next() => {
                    let Some(hotplug_event) = hotplug_event else {
                        return Err(Error::Transport(UsbError::WatcherReturnedNone.into()))
                    };
                    if let HotplugEvent::Connected(di) = hotplug_event {
                        if filter.matches_nusb(&di)? {
                            // as per nusb docs, must wait a bit on Windows after getting watched device, otherwise connection fails
                            #[cfg(target_os = "windows")]
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await; // TODO: is 10ms enough on slow Windows VM?

                            return Ok(di)
                        }
                    }

                    let now = Instant::now();
                    let dt = now.duration_since(wait_started);
                    if dt > timeout {
                        return Err(Error::DeviceNotFound)
                    } else {
                        timeout -= dt;
                    }
                }
            }
        },
        OnError::KeepRetrying => {
            while let Some(hotplug_event) = watch.next().await {
                if let HotplugEvent::Connected(di) = hotplug_event {
                    if filter.matches_nusb(&di)? {
                        return Ok(di);
                    }
                }
            }
            Err(Error::Transport(UsbError::WatcherReturnedNone.into()))
        }
    }
}
