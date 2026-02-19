use std::net::IpAddr;

#[derive(Clone, Debug)]
pub struct DeviceFilter {
    pub kind: DeviceFilterKind,
    // /// If a server process dedicated to the targeted device is already running, connect through it, instead of directly
    // pub allow_ipc: bool,
    // /// Spawn an IPC process, that will establish an actual connection to the selected device and connect through it
    // pub require_ipc: bool,
}

#[derive(Clone, Debug)]
pub enum DeviceFilterKind {
    WebSocket {
        addr: IpAddr,
        port: u16,
        path: String,
    },
    UDP {
        addr: IpAddr,
        port: u16,
    },
    UsbFlexible {
        vid: Option<u16>,
        pid: Option<u16>,
        manufacturer_contains: Option<&'static str>,
        product_contains: Option<&'static str>,
    },
    UsbVidPid {
        vid: u16,
        pid: u16,
    },
    UsbVidPidAndSerial {
        vid: u16,
        pid: u16,
        serial: String,
    },
    UsbPath {
        bus_id: String,
        port_chain: Vec<u8>,
    },
    Serial {
        serial: String,
    },
}

impl DeviceFilter {
    pub fn usb_vid_pid(vid: u16, pid: u16) -> DeviceFilter {
        Self {
            kind: DeviceFilterKind::UsbVidPid { vid, pid },
        }
    }

    pub fn vhrd_usb_can() -> DeviceFilter {
        Self {
            kind: DeviceFilterKind::UsbFlexible {
                vid: None,
                pid: None,
                manufacturer_contains: Some("vhrd"),
                product_contains: Some("can"),
            },
        }
    }

    pub fn vhrd_usb_io() -> DeviceFilter {
        Self {
            kind: DeviceFilterKind::UsbFlexible {
                vid: None,
                pid: None,
                manufacturer_contains: Some("vhrd"),
                product_contains: Some("io"),
            },
        }
    }

    pub fn as_web_socket(&self) -> Option<(IpAddr, u16, String)> {
        if let DeviceFilterKind::WebSocket { addr, port, path } = self.kind.clone() {
            Some((addr, port, path))
        } else {
            None
        }
    }

    pub fn as_udp(&self) -> Option<(IpAddr, u16)> {
        if let DeviceFilterKind::UDP { addr, port } = self.kind {
            Some((addr, port))
        } else {
            None
        }
    }

    #[cfg(feature = "nusb")]
    pub fn from_nusb(device_info: &nusb::DeviceInfo) -> Self {
        Self {
            kind: DeviceFilterKind::UsbPath {
                bus_id: device_info.bus_id().to_string(),
                port_chain: device_info.port_chain().to_vec(),
            },
        }
    }

    #[cfg(feature = "nusb")]
    pub fn matches_nusb(&self, device_info: &nusb::DeviceInfo) -> Result<bool, crate::Error> {
        let matches = match &self.kind {
            DeviceFilterKind::UsbVidPid { vid, pid } => {
                device_info.vendor_id() == *vid && device_info.product_id() == *pid
            }
            DeviceFilterKind::UsbVidPidAndSerial { vid, pid, serial } => {
                if device_info.vendor_id() != *vid || device_info.product_id() != *pid {
                    false
                } else if let Some(s) = device_info.serial_number() {
                    s == serial
                } else {
                    false
                }
            }
            DeviceFilterKind::Serial { serial } => {
                if let Some(s) = device_info.serial_number() {
                    s == serial
                } else {
                    false
                }
            }
            DeviceFilterKind::UsbFlexible {
                vid,
                pid,
                manufacturer_contains,
                product_contains,
            } => {
                if let Some(vid) = *vid
                    && device_info.vendor_id() != vid
                {
                    return Ok(false);
                }
                if let Some(pid) = *pid
                    && device_info.product_id() != pid
                {
                    return Ok(false);
                }
                if let Some(manufacturer_contains) = manufacturer_contains {
                    let manufacturer_contains = manufacturer_contains.to_lowercase();
                    let Some(manufacturer) = device_info.manufacturer_string() else {
                        return Ok(false);
                    };
                    if !manufacturer.contains(&manufacturer_contains) {
                        return Ok(false);
                    }
                }
                if let Some(product_contains) = product_contains {
                    let product_contains = product_contains.to_lowercase();
                    let Some(product_string) = device_info.product_string() else {
                        return Ok(false);
                    };
                    if !product_string.contains(&product_contains) {
                        return Ok(false);
                    }
                }
                true
            }
            DeviceFilterKind::UsbPath { bus_id, port_chain } => {
                device_info.bus_id() == bus_id && device_info.port_chain() == port_chain
            }
            u => {
                return Err(crate::Error::Transport(format!(
                    "unsupported filter for USB host: {:?}",
                    u
                )));
            }
        };
        Ok(matches)
    }
}
