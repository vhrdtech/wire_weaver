use crate::{Error, OnError, UsbDeviceFilter};
use futures_lite::StreamExt;
use nusb::hotplug::HotplugEvent;
use nusb::{DeviceInfo, Interface};
use std::time::Instant;
use tracing::{error, trace};

pub(crate) async fn connect(
    filter: UsbDeviceFilter,
    mut timeout: OnError,
) -> Result<(Interface, DeviceInfo), Error> {
    let wait_started = Instant::now();
    loop {
        let di = nusb::list_devices()?.find(|d| apply_filter(d, &filter));
        let di = match di {
            Some(di) => di,
            None => {
                if timeout == OnError::ExitImmediately {
                    return Err(Error::Timeout);
                } else {
                    wait_device(&filter, timeout).await?
                }
            }
        };
        trace!("connecting to USB device: {di:?}");
        let interface = {
            let dev = di.open()?;
            dev.claim_interface(0)
        };
        match interface {
            Ok(interface) => {
                return Ok((interface, di));
            }
            Err(e) => match &mut timeout {
                OnError::ExitImmediately => {
                    error!("device {di:?} open failed: {e:?}, no timeout, bailing");
                    return Err(Error::Timeout);
                }
                OnError::RetryFor { timeout, .. } => {
                    let now = Instant::now();
                    let dt = now.duration_since(wait_started);
                    if dt > *timeout {
                        error!("device {di:?} open failed: {e:?}, timeout expired, bailing");
                        return Err(Error::Timeout);
                    } else {
                        *timeout = *timeout - dt;
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

async fn wait_device(filter: &UsbDeviceFilter, timeout: OnError) -> Result<DeviceInfo, Error> {
    let mut watch = nusb::watch_devices()?;
    if let Some(di) = nusb::list_devices()?.find(|d| apply_filter(d, filter)) {
        return Ok(di);
    };
    trace!("waiting for USB device to connect...");
    match timeout {
        OnError::ExitImmediately => Err(Error::Timeout),
        OnError::RetryFor { mut timeout, .. } => loop {
            let wait_started = Instant::now();
            tokio::select! {
                _ = tokio::time::sleep(timeout) => {
                    return Err(Error::Timeout)
                }
                hotplug_event = watch.next() => {
                    let Some(hotplug_event) = hotplug_event else {
                        return Err(Error::WatcherReturnedNone)
                    };
                    if let HotplugEvent::Connected(di) = hotplug_event {
                        if apply_filter(&di, filter) {
                            return Ok(di)
                        }
                    }

                    let now = Instant::now();
                    let dt = now.duration_since(wait_started);
                    if dt > timeout {
                        return Err(Error::Timeout)
                    } else {
                        timeout = timeout - dt;
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
            Err(Error::WatcherReturnedNone)
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
        UsbDeviceFilter::AnyVhrdTechCanBus => {
            let Some(product_string) = device_info.product_string() else {
                return false;
            };
            device_info.vendor_id() == 0xc0de
                && device_info.product_id() == 0xcafe
                && product_string.contains("CAN")
        }
    }
}
