use std::{net::IpAddr, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use semver::VersionReq;
use strum_macros::EnumDiscriminants;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_self::ApiBundleOwned;
use ww_version::{FullVersionOwned, VersionOwned};

use crate::device_info::ApiHash;

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
#[derive(Clone, Debug, Default)]
pub struct ClientConfig {
    pieces: Vec<ConfigPiece>,
    priority: String,
    cmd_queue_size: Option<usize>,
    introspect: Option<(Vec<u8>, ApiHash, ApiHash)>,
    default_timeout: Option<Duration>,
    client_version: Option<Box<FullVersionOwned>>,
}

pub(crate) struct ValidatedConfig {
    pub(crate) pieces: Vec<ConfigPiece>,
    priority: Vec<InterfaceKind>,
    pub(crate) cmd_queue_size: usize,
    pub(crate) default_timeout: Duration,
    pub(crate) client_version: Box<FullVersionOwned>,
    pub(crate) introspect: Option<IntrospectBundle>,
}

#[derive(Clone)]
pub(crate) struct IntrospectBundle {
    pub(crate) api_bundle: Arc<ApiBundleOwned>,
    pub(crate) hash_no_docs: ApiHash,
    pub(crate) hash_with_docs: ApiHash,
}

#[derive(Debug, PartialEq, Eq)]
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
pub(crate) enum ConfigPiece {
    /// Connect to a USB device with the specified VID:PID
    UsbVidPid { vid: u16, pid: u16 },
    /// Connect to a USB device at the specified path (bus_number + port chain)
    UsbPath { bus_id: String, port_chain: Vec<u8> },
    /// Connect to a USB device, other filter pieces are required to select which one.
    Usb,
    /// Negate previous USB related filters or exclude USB from the interfaces to try.
    NoUsb,

    /// Connect to a networked device via WebSocket
    WebSocketAddr {
        addr: IpAddr,
        port: u16,
        path: String,
    },
    /// Connect to a network device via WebSocket, other filter pieces are required to select which one.
    WebSocket,
    /// Negate previous WebSocket related filters or exclude WebSocket from the interfaces to try.
    NoWebSocket,

    /// Connect to a networked device via UDP
    UdpAddr { addr: IpAddr, port: u16 },
    /// Connect to a network device via UDP, other filter pieces are required to select which one.
    Udp,
    /// Negate previous UDP related filters or exclude WebSocket from the interfaces to try.
    NoUdp,

    /// Connect to a device running in another process via IPC interface (iceoryx2).
    /// Used to split multiplex multiple clients to one USB device for example.
    /// Server claims USB interface, clients connect to it via ipc or network.
    /// Can also be used for testing purposes or to create virtual devices.
    Ipc,
    /// Negate previous IPC related filters or exclude IPC from the interfaces to try.
    NoIpc,

    /// Connec to a device running in the same process via lock-free channel.
    /// Can be used for testing purposes or to create virtual devices.
    InProcess,
    /// Negate previous InProcess related filters or exclude InProcess from the interfaces to try.
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
impl From<nusb::DeviceInfo> for ClientConfig {
    fn from(nusb_info: nusb::DeviceInfo) -> Self {
        ClientConfig {
            pieces: vec![ConfigPiece::UsbPath {
                bus_id: nusb_info.bus_id().to_string(),
                port_chain: nusb_info.port_chain().to_vec(),
            }],
            ..Default::default()
        }
    }
}

impl ClientConfig {
    pub fn new() -> Self {
        Default::default()
    }

    // pub fn from_pieces(pieces: impl IntoIterator<Item = ConfigPiece>) -> Self {
    //     Self {
    //         pieces: pieces.into_iter().collect(),
    //         ..Default::default()
    //     }
    // }

    pub(crate) fn validate(self) -> Result<ValidatedConfig> {
        let s = self;
        // s.canonicalize();
        let introspect = if let Some((ww_self_bytes, hash_no_docs, hash_with_docs)) = s.introspect {
            let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
            Some(IntrospectBundle {
                api_bundle: Arc::new(api_bundle),
                hash_no_docs,
                hash_with_docs,
            })
        } else {
            None
        };
        let cmd_queue_size = s.cmd_queue_size.unwrap_or(crate::DEFAULT_CMD_QUEUE_SIZE);
        if cmd_queue_size < 1 || cmd_queue_size > 65_534 {
            bail!("Wrong cmd queue size of {cmd_queue_size}");
        }
        // connecting as dynamic client (introspect device) if no client version specified
        let client_version = s.client_version.unwrap_or(Box::new(FullVersionOwned::new(
            "".into(),
            VersionOwned::new(0, 1, 0),
        )));
        Ok(ValidatedConfig {
            pieces: s.pieces,
            priority: vec![],
            cmd_queue_size,
            default_timeout: s.default_timeout.unwrap_or(crate::DEFAULT_REQUEST_TIMEOUT),
            client_version,
            introspect,
        })
    }

    /// Set connection priority list when multiple options are available, e.g., "net>usb>ipc"
    pub fn prio(&mut self, priority: String) -> &mut Self {
        self.priority = priority;
        self
    }

    /// Select USB device by VID:PID numbers
    pub fn usb_vid_pid(self, vid: u16, pid: u16) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::UsbVidPid { vid, pid });
        f
    }

    /// Select USB device by bus name and port chain
    pub fn usb_port_chain(self, bus_id: String, port_chain: Vec<u8>) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::UsbPath { bus_id, port_chain });
        f
    }

    /// Consider USB devices as a potential connection targets
    pub fn usb(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::Usb);
        f
    }

    /// Do not consider USB devices as a potential connection targets
    pub fn no_usb(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::NoUsb);
        f
    }

    /// CommandSender queue size, limits the amount of simulatenous requests in-flight.
    /// Default is 8192, using more than 65_534 will lead to blocking if reached.
    pub fn cmd_queue_size(self, size: usize) -> Self {
        let mut f = self;
        f.cmd_queue_size = Some(size);
        f
    }

    pub fn introspect(
        self,
        ww_self_bytes: &[u8],
        hash_no_docs: &[u8],
        hash_with_docs: &[u8],
    ) -> Self {
        let mut c = self;
        c.introspect = Some((
            ww_self_bytes.to_vec(),
            ApiHash(hash_no_docs.to_vec()),
            ApiHash(hash_with_docs.to_vec()),
        ));
        c
    }

    pub fn default_timeout(self, timeout: Duration) -> Self {
        let mut c = self;
        c.default_timeout = Some(timeout);
        c
    }

    // pub(crate) fn canonicalize(&mut self) {
    //     #[cfg(feature = "usb")]
    //     self.canonicalize_inner(
    //         &[
    //             ConfigPieceDiscriminants::Usb,
    //             ConfigPieceDiscriminants::UsbVidPid,
    //             ConfigPieceDiscriminants::UsbPath,
    //         ],
    //         ConfigPieceDiscriminants::NoUsb,
    //     );
    // }

    // fn canonicalize_inner(
    //     &mut self,
    //     opt_in: &[ConfigPieceDiscriminants],
    //     opt_out: ConfigPieceDiscriminants,
    // ) {
    //     let mut opt_in_at = vec![];
    //     let mut remove_at = vec![];
    //     for (idx, p) in self.pieces.iter().enumerate() {
    //         let kind = ConfigPieceDiscriminants::from(p);
    //         if opt_in.contains(&kind) {
    //             opt_in_at.push(idx);
    //         }
    //         if kind == opt_out {
    //             remove_at.extend(opt_in_at.drain(..));
    //             remove_at.push(idx);
    //         }
    //     }
    //     let mut idx = 0;
    //     self.pieces.retain(|_| {
    //         let retain = !remove_at.contains(&idx);
    //         idx += 1;
    //         retain
    //     });
    // }
}

impl ValidatedConfig {
    pub(crate) fn is_usb(&self) -> bool {
        self.is_opted_in(
            &[
                ConfigPieceDiscriminants::UsbPath,
                ConfigPieceDiscriminants::UsbVidPid,
                ConfigPieceDiscriminants::Usb,
            ],
            ConfigPieceDiscriminants::NoUsb,
        )
    }

    pub(crate) fn interfaces(&self) -> Vec<InterfaceKind> {
        let mut interfaces = vec![];
        if self.is_usb() {
            interfaces.push(InterfaceKind::Usb);
        }
        // TODO: sort by priority
        interfaces
    }

    pub(crate) fn manufacturers_contains(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let ConfigPiece::ManufacturerContains { substring } = p {
                Some(substring.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn products_contains(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let ConfigPiece::ProductContains { substring } = p {
                Some(substring.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn serials_eq(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let ConfigPiece::SerialEq { serial } = p {
                Some(serial.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn user_labels_eq(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| {
            if let ConfigPiece::UserLabelEq { user_label } = p {
                Some(user_label.as_str())
            } else {
                None
            }
        })
    }

    pub(crate) fn implements_api(&self) -> impl Iterator<Item = (&str, &VersionReq)> {
        self.pieces.iter().filter_map(|p| {
            if let ConfigPiece::ImplementsApi {
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

    fn is_opted_in(
        &self,
        opt_in: &[ConfigPieceDiscriminants],
        opt_out: ConfigPieceDiscriminants,
    ) -> bool {
        let find_rev = |d: &[ConfigPieceDiscriminants]| {
            self.pieces
                .iter()
                .rev()
                .enumerate()
                .find(|(_, p)| d.contains(&ConfigPieceDiscriminants::from(*p)))
                .map(|(idx, _)| idx)
        };
        let opted_out = find_rev(&[opt_out]);
        let opted_in = find_rev(opt_in);
        match (opted_out, opted_in) {
            (None, None) => false,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (Some(opted_out), Some(opted_in)) => opted_in < opted_out,
        }
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
        let f = ClientConfig::new().validate().unwrap();
        assert_eq!(f.is_usb(), false); // unless opted-in, interface is not considered

        let f = ClientConfig::new()
            .usb_vid_pid(0x1, 0x2)
            .usb()
            .validate()
            .unwrap();
        assert_eq!(
            f.pieces,
            vec![
                ConfigPiece::UsbVidPid { vid: 0x1, pid: 0x2 },
                ConfigPiece::Usb
            ]
        );
        assert_eq!(f.is_usb(), true);

        let f = ClientConfig::new().usb().validate().unwrap();
        assert_eq!(f.pieces, vec![ConfigPiece::Usb]);
        assert_eq!(f.is_usb(), true);

        let f = ClientConfig::new().usb().no_usb().validate().unwrap();
        assert_eq!(f.pieces, vec![ConfigPiece::Usb, ConfigPiece::NoUsb]);
        assert_eq!(f.is_usb(), false);

        let f = ClientConfig::new().usb().no_usb().usb().validate().unwrap();
        assert_eq!(
            f.pieces,
            vec![ConfigPiece::Usb, ConfigPiece::NoUsb, ConfigPiece::Usb]
        );
        assert_eq!(f.is_usb(), true);
    }
}
