use crate::{UsbDeviceFilter, UsbError};
use futures_lite::StreamExt;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceInfo, Interface};
use std::time::Instant;
use tracing::{error, trace};
use wire_weaver_client_common::{Error, OnError};

pub(crate) async fn connect(
    filter: UsbDeviceFilter,
    mut timeout: OnError,
) -> Result<(Interface, DeviceInfo), Error<UsbError>> {
    let wait_started = Instant::now();
    loop {
        let di = nusb::list_devices()
            .map_err(|e| Error::Transport(e.into()))?
            .find(|d| apply_filter(d, &filter));
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
        let interface = {
            let dev = di.open().map_err(|e| Error::Transport(e.into()))?;
            dev.claim_interface(0)
        };
        match interface {
            Ok(interface) => {
                return Ok((interface, di));
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

async fn wait_device(
    filter: &UsbDeviceFilter,
    timeout: OnError,
) -> Result<DeviceInfo, Error<UsbError>> {
    let mut watch = nusb::watch_devices().map_err(|e| Error::Transport(e.into()))?;
    if let Some(di) = nusb::list_devices()
        .map_err(|e| Error::Transport(e.into()))?
        .find(|d| apply_filter(d, filter))
    {
        return Ok(di);
    };
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
                        return Err(Error::Transport(UsbError::WatcherReturnedNone))
                    };
                    if let HotplugEvent::Connected(di) = hotplug_event {
                        if apply_filter(&di, filter) {
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
                    if apply_filter(&di, filter) {
                        return Ok(di);
                    }
                }
            }
            Err(Error::Transport(UsbError::WatcherReturnedNone))
        }
    }
}

fn apply_filter(device_info: &DeviceInfo, filter: &UsbDeviceFilter) -> bool {
    match filter {
        UsbDeviceFilter::VidPid { vid, pid } => {
            device_info.vendor_id() == *vid && device_info.product_id() == *pid
        }
        UsbDeviceFilter::VidPidAndSerial { vid, pid, serial } => {
            if device_info.vendor_id() != *vid || device_info.product_id() != *pid {
                false
            } else if let Some(s) = device_info.serial_number() {
                s == serial
            } else {
                false
            }
        }
        UsbDeviceFilter::Serial { serial } => {
            if let Some(s) = device_info.serial_number() {
                s == serial
            } else {
                false
            }
        }
        UsbDeviceFilter::AnyVhrdTechCanBus | UsbDeviceFilter::AnyVhrdTechIo => {
            let Some(manufacturer) = device_info.manufacturer_string() else {
                return false;
            };
            let Some(product_string) = device_info.product_string() else {
                return false;
            };
            if !manufacturer.to_lowercase().contains("vhrd") {
                return false;
            }
            match filter {
                UsbDeviceFilter::AnyVhrdTechCanBus => product_string.contains("CAN"),
                UsbDeviceFilter::AnyVhrdTechIo => product_string.contains("IO"),
                _ => unreachable!(),
            }
        }
    }
}
