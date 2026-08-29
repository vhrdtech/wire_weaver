#[cfg(feature = "net")]
use std::net::IpAddr;

use anyhow::Result;
use semver::VersionReq;
use strum_macros::EnumDiscriminants;

use crate::options::DeviceFilterPieceDiscriminants::UserLabelEq;

/// Configuration of device enumuration, selection and connection.
/// Flexible filters allow for many different scenarios:
/// - Specific device selection
///     - USB by VID:PID or port chain
///     - Network by IP:PORT
///     - CAN by specifying network name or providing USB or Network filters for bridges
/// - Selection based on:
///     - Manufacturer and/or serial strings
///     - Custom user label (nickname)
///     - Whether device implements certain API protocol
/// - Selection from multiple options:
///     - Priority system, e.g., "ipc>usb>ws"
///     - "Adding" option ('Usb', 'WebSocket', 'Udp', etc.)
///     - "Removing" option ('NoUsb', 'NoWebSocket', 'NoUdp', etc.)
/// - Builder pattern that plays nicely with specific device crates
#[derive(Clone, Debug)]
pub struct Options {
    pub(crate) pieces: Vec<OptionPiece>,
    priority: String,
    cmd_queue_size: usize,
}

struct ValidatedOptions {
    pub(crate) pieces: Vec<OptionPiece>,
    priority: Vec<InterfaceKind>,
    cmd_queue_size: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            pieces: vec![],
            priority: Default::default(),
            cmd_queue_size: 8_192,
        }
    }
}

pub(crate) enum InterfaceKind {
    Usb,
    WebSocket,
    Udp,
    Ipc,
    InProcess,
    Can,
    Uart,
    I2c,
    Rtt,
}

#[derive(Clone, Debug, PartialEq, Eq, EnumDiscriminants)]
pub(crate) enum OptionPiece {
    /// Connect to a USB device with the specified VID:PID
    #[cfg(feature = "usb")]
    UsbVidPid { vid: u16, pid: u16 },
    /// Connect to a USB device at the specified path (bus_number + port chain)
    #[cfg(feature = "usb")]
    UsbPath { bus_id: String, port_chain: Vec<u8> },
    /// Connect to a USB device, other filter pieces are required to select which one.
    #[cfg(feature = "usb")]
    Usb,
    /// Negate previous USB related filters or exclude USB from the interfaces to try.
    #[cfg(feature = "usb")]
    NoUsb,

    /// Connect to a networked device via WebSocket
    #[cfg(feature = "net")]
    WebSocketAddr {
        addr: IpAddr,
        port: u16,
        path: String,
    },
    /// Connect to a network device via WebSocket, other filter pieces are required to select which one.
    #[cfg(feature = "net")]
    WebSocket,
    /// Negate previous WebSocket related filters or exclude WebSocket from the interfaces to try.
    #[cfg(feature = "net")]
    NoWebSocket,

    /// Connect to a networked device via UDP
    #[cfg(feature = "net")]
    UdpAddr { addr: IpAddr, port: u16 },
    /// Connect to a network device via UDP, other filter pieces are required to select which one.
    #[cfg(feature = "net")]
    Udp,
    /// Negate previous UDP related filters or exclude WebSocket from the interfaces to try.
    #[cfg(feature = "net")]
    NoUdp,

    /// Connect to a device running in another process via IPC interface (iceoryx2).
    /// Used to split multiplex multiple clients to one USB device for example.
    /// Server claims USB interface, clients connect to it via ipc or network.
    /// Can also be used for testing purposes or to create virtual devices.
    #[cfg(feature = "ipc")]
    Ipc,
    /// Negate previous IPC related filters or exclude IPC from the interfaces to try.
    #[cfg(feature = "ipc")]
    NoIpc,

    /// Connec to a device running in the same process via lock-free channel.
    /// Can be used for testing purposes or to create virtual devices.
    #[cfg(feature = "in_process")]
    InProcess,
    /// Negate previous InProcess related filters or exclude InProcess from the interfaces to try.
    #[cfg(feature = "in_process")]
    NoInProcess,

    /// Filter out a device with the specified serial number. Ignoring case.
    SerialEq { serial: String },
    /// Filter out a device with the specified user label. Ignoring case.
    /// User labels can be assigned via [ww](https://vhrd.tech/TODO) CLI tool or product-specific CLI, GUI or API.
    UserLabelEq { user_label: String },
    /// Filter out a device whose manufacturer string contains the substring. Igoring case.
    ManufacturerContains { substring: String },
    /// Filter out a device whose product string contains the substring. Ignoring case.
    ProductContains { substring: String },
    /// Filter out a device that implements specified WireWeaver API.
    ImplementsApi {
        api_gid: String,
        version_req: VersionReq,
    },
}

#[cfg(feature = "usb")]
impl From<wire_weaver_usb_host::nusb::DeviceInfo> for Options {
    fn from(nusb_info: wire_weaver_usb_host::nusb::DeviceInfo) -> Self {
        Options {
            pieces: vec![OptionPiece::UsbPath {
                bus_id: nusb_info.bus_id().to_string(),
                port_chain: nusb_info.port_chain().to_vec(),
            }],
            ..Default::default()
        }
    }
}

impl Options {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn from_pieces(pieces: impl IntoIterator<Item = OptionPiece>) -> Self {
        Self {
            pieces: pieces.into_iter().collect(),
            ..Default::default()
        }
    }

    pub(crate) fn validate(self) -> Result<ValidatedOptions> {
        self.canonicalize();
        Ok(ValidatedOptions {
            pieces: self.pieces,
            priority: vec![],
            cmd_queue_size: self.cmd_queue_size,
        })
    }

    /// Set connection priority list when multiple options are available, e.g., "net>usb>ipc"
    pub fn prio(&mut self, priority: String) -> &mut Self {
        self.priority = priority;
        self
    }

    pub(crate) fn manufacturers_contains(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let OptionPiece::ManufacturerContains { substring } = p {
                Some(substring.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn products_contains(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let OptionPiece::ProductContains { substring } = p {
                Some(substring.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn serials_eq(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let OptionPiece::SerialEq { serial } = p {
                Some(serial.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn user_labels_eq(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let OptionPiece::UserLabelEq { user_label } = p {
                Some(user_label.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn implements_api(&self) -> impl Iterator<Item = (&str, &VersionReq)> {
        self.pieces.iter().filter_map(|p| {
            if let OptionPiece::ImplementsApi {
                api_gid,
                version_req,
            } = p
            {
                Some((api_gid.as_str(), version_req))
            } else {
                None
            }
        })
    }

    /// Select USB device by VID:PID numbers
    #[cfg(feature = "usb")]
    pub fn usb_vid_pid(self, vid: u16, pid: u16) -> Self {
        let mut f = self;
        f.pieces.push(OptionPiece::UsbVidPid { vid, pid });
        f
    }

    /// Select USB device by bus name and port chain
    #[cfg(feature = "usb")]
    pub fn usb_port_chain(self, bus_id: String, port_chain: Vec<u8>) -> Self {
        let mut f = self;
        f.pieces.push(OptionPiece::UsbPath { bus_id, port_chain });
        f
    }

    /// Consider USB devices as a potential connection targets
    #[cfg(feature = "usb")]
    pub fn usb(self) -> Self {
        let mut f = self;
        f.pieces.push(OptionPiece::Usb);
        f
    }

    /// Do not consider USB devices as a potential connection targets
    #[cfg(feature = "usb")]
    pub fn no_usb(self) -> Self {
        let mut f = self;
        f.pieces.push(OptionPiece::NoUsb);
        f
    }

    /// CommandSender queue size, limits the amount of simulatenous requests in-flight.
    /// Default is 8192, using more than 65_534 will lead to blocking if reached.
    pub fn cmd_queue_size(self, size: usize) -> Self {
        let mut f = self;
        f.cmd_queue_size = size;
        f
    }

    pub(crate) fn canonicalize(&mut self) {
        #[cfg(feature = "usb")]
        self.canonicalize_inner(
            &[
                DeviceFilterPieceDiscriminants::Usb,
                DeviceFilterPieceDiscriminants::UsbVidPid,
                DeviceFilterPieceDiscriminants::UsbPath,
            ],
            DeviceFilterPieceDiscriminants::NoUsb,
        );
    }

    fn canonicalize_inner(
        &mut self,
        opt_in: &[DeviceFilterPieceDiscriminants],
        opt_out: DeviceFilterPieceDiscriminants,
    ) {
        let mut opt_in_at = vec![];
        let mut remove_at = vec![];
        for (idx, p) in self.pieces.iter().enumerate() {
            let kind = DeviceFilterPieceDiscriminants::from(p);
            if opt_in.contains(&kind) {
                opt_in_at.push(idx);
            }
            if kind == opt_out {
                remove_at.extend(opt_in_at.drain(..));
                remove_at.push(idx);
            }
        }
        let mut idx = 0;
        self.pieces.retain(|_| {
            let retain = !remove_at.contains(&idx);
            idx += 1;
            retain
        });
    }
}

impl ValidatedOptions {
    pub(crate) fn is_usb(&self) -> bool {
        self.pieces.iter().any(|p| {
            matches!(
                p,
                OptionPiece::UsbVidPid { .. } | OptionPiece::UsbPath { .. } | OptionPiece::Usb
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Some of the tests may seem silly, but filters can be created in device driver crates
    // and then altered by an end user.
    // Also user may depend on two driver crates, with one using e.g., only WebSocket and the other only USB.
    // In which case two interface features will be enabled and if not for this filter system,
    // it would break the mess with device selection process.
    #[test]
    fn usb() {
        let f = Options::new().validate().unwrap();
        assert_eq!(f.is_usb(), false); // unless opted-in, interface is not considered

        let mut f = Options::new()
            .usb_vid_pid(0x1, 0x2)
            .usb()
            .validate()
            .unwrap();
        f.canonicalize();
        assert_eq!(
            f.pieces,
            vec![
                OptionPiece::UsbVidPid { vid: 0x1, pid: 0x2 },
                OptionPiece::Usb
            ]
        );
        assert_eq!(f.is_usb(), true);

        let mut f = Options::new().usb().validate().unwrap();
        f.canonicalize();
        assert_eq!(f.pieces, vec![OptionPiece::Usb]);
        assert_eq!(f.is_usb(), true);

        let mut f = Options::new().usb().no_usb().validate().unwrap();
        f.canonicalize();
        assert_eq!(f.pieces, vec![]);
        assert_eq!(f.is_usb(), false);

        let mut f = Options::new().usb().no_usb().usb().validate().unwrap();
        f.canonicalize();
        assert_eq!(f.pieces, vec![OptionPiece::Usb]);
        assert_eq!(f.is_usb(), true);
    }
}
