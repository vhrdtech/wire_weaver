use std::{net::IpAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use semver::VersionReq;
use strum_macros::EnumDiscriminants;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_self::ApiBundleOwned;
use ww_version::{ApiHashPairOwned, FullVersionOwned, VersionOwned};

use crate::{DeviceInfo, SeqTy};

/// Configuration of device enumeration, selection and connection.
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
    introspect_client: Option<(Vec<u8>, ApiHashPairOwned)>,
    default_timeout: Option<Duration>,
    client_version: Option<Box<FullVersionOwned>>,
    max_seq: Option<SeqTy>,
}

pub(crate) struct ValidatedConfig {
    pub(crate) pieces: Vec<ConfigPiece>,
    #[allow(dead_code)] // TODO: implement connection priority
    priority: Vec<InterfaceKind>,
    pub(crate) cmd_queue_size: usize,
    pub(crate) default_timeout: Duration,
    pub(crate) client_version: Box<FullVersionOwned>,
    pub(crate) max_seq: SeqTy,
    pub(crate) introspect_client: Option<IntrospectBundle>,
}

#[derive(Clone, Debug)]
pub struct IntrospectBundle {
    /// Full API, with traits and types left out of the introspection data (known from snapshots) put back.
    pub api_bundle: Arc<ApiBundleOwned>,
    pub api_hash: ApiHashPairOwned,
    /// Introspection data as sent by a device (or embedded into a client), before traits and types left out of it
    /// were put back.
    pub sent_api_bundle: Arc<ApiBundleOwned>,
    /// Size of the introspection data as sent by a device.
    pub sent_size: usize,
}

impl IntrospectBundle {
    /// Put back traits and types that `sent_api_bundle` left out, because they are known from snapshots.
    pub(crate) fn from_sent(
        sent_api_bundle: ApiBundleOwned,
        api_hash: ApiHashPairOwned,
        sent_size: usize,
    ) -> Self {
        let mut api_bundle = sent_api_bundle.clone();
        crate::client::introspect::inline_known(&mut api_bundle);
        IntrospectBundle {
            api_bundle: Arc::new(api_bundle),
            api_hash,
            sent_api_bundle: Arc::new(sent_api_bundle),
            sent_size,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InterfaceKind {
    Usb,
    WebSocket,
    Udp,
    Ipc,
    InProcess,
    Rtt,
    // Can,
    // Uart,
    // I2c,
}

#[derive(Clone, Debug, PartialEq, Eq, EnumDiscriminants)]
pub(crate) enum ConfigPiece {
    /// Connect to a USB device with the specified VID:PID
    UsbVidPid { vid: u16, pid: u16 },
    /// Connect to a USB device at the specified path (bus_number + port chain)
    UsbPath { bus_id: String, port_chain: Vec<u8> },
    /// Connect to a USB device using WireWeaver protocol, other filter pieces are required to select which one.
    Usb,
    /// Negate previous USB related filters or exclude USB from the interfaces to try.
    /// Note that this excludes WireWeaver USB protocol, but not other adapters working over it:
    /// `.usb_vid_pid(1, 2).no_ww_usb().can()` will try to connect over CAN Bus using USB-CAN bridge
    NoWwUsb,

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
    /// Can be used for testing purposes or to create virtual devices.
    IpcPath { path: String },
    /// Connect to a device running in another process via IPC interface (iceoryx2).
    /// Find which one using USB filters.
    /// Used to split multiplex multiple clients to one USB device for example.
    /// Server claims USB interface, clients connect to it via ipc or network.
    UsbOverIpc,
    /// Consider IPC as a connection interface.
    Ipc,
    /// Negate previous IPC related filters or exclude IPC from the interfaces to try.
    NoIpc,

    /// Connect to a device running in the same process via lock-free channel.
    /// Can be used for testing purposes or to create virtual devices.
    InProcessPath { path: String },
    /// Negate previous InProcess related filters or exclude InProcess from the interfaces to try.
    NoInProcess,

    /// Connect to a device over RTT over JTAG/SWD, WireWeaver USB is not tried then.
    /// VID:PID and serial filters after this piece select which probe to use if there are several of them.
    Rtt {
        target: String,
        protocol: (),
        speed_hz: Option<u32>,
    },
    /// Where the RTT control block is, instead of scanning all RAM for it (slow on chips with a lot of RAM).
    RttControlBlock(RttControlBlock),

    /// Filter out a device with the specified serial number. Ignoring case.
    SerialEq { serial: String },
    /// Filter out a device whose serial number contains the substring. Ignoring case.
    SerialContains { substring: String },
    /// Filter out a device with the specified user label. Ignoring case.
    /// User labels are set by the firmware (see [USB transport](https://ww.vhrd.tech/transport/usb/)), `ww list` shows them.
    UserLabelEq { user_label: String },
    /// Filter out a device whose manufacturer string contains the substring. Ignoring case.
    ManufacturerContains { substring: String },
    /// Filter out a device whose product string contains the substring. Ignoring case.
    ProductContains { substring: String },
    /// Filter out a device that implements specified WireWeaver API.
    ImplementsApi {
        api_gid: String,
        version_req: VersionReq,
    },
}

/// See [ClientConfig::rtt_elf] and [ClientConfig::rtt_control_block_at].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RttControlBlock {
    Elf(PathBuf),
    Address(u64),
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
        let introspect_client = if let Some((ww_self_bytes, api_hash)) = s.introspect_client {
            // same as a device sends, with known traits and types left out
            let sent_api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
            Some(IntrospectBundle::from_sent(
                sent_api_bundle,
                api_hash,
                ww_self_bytes.len(),
            ))
        } else {
            None
        };
        let max_seq = s.max_seq.unwrap_or(crate::DEFAULT_MAX_SEQ);
        if max_seq == 0 {
            bail!("max_seq must be at least 1");
        }
        let cmd_queue_size = s.cmd_queue_size.unwrap_or(crate::DEFAULT_CMD_QUEUE_SIZE);
        if !(1..=65_534).contains(&cmd_queue_size) {
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
            max_seq,
            introspect_client,
        })
    }

    /// Whether the device passes all filters of this config (serial, label, product, API, VID:PID, etc.).
    /// Filters of the same kind are alternatives, filters of different kinds are all required.
    pub fn matches(&self, device: &DeviceInfo) -> bool {
        device.is_matching(&self.pieces)
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
    pub fn no_ww_usb(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::NoWwUsb);
        f
    }

    /// Select WebSocket device by IP:PORT/PATH
    pub fn websocket_addr(self, addr: IpAddr, port: u16, path: String) -> Self {
        let mut f = self;
        f.pieces
            .push(ConfigPiece::WebSocketAddr { addr, port, path });
        f
    }

    /// Consider WebSocket devices as a potential connection targets
    pub fn websocket(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::WebSocket);
        f
    }

    /// Do not consider WebSocket devices as a potential connection targets
    pub fn no_websocket(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::NoWebSocket);
        f
    }

    /// Select UDP device by IP:PORT
    pub fn udp_addr(self, addr: IpAddr, port: u16) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::UdpAddr { addr, port });
        f
    }

    /// Consider UDP devices as a potential connection targets
    pub fn udp(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::Udp);
        f
    }

    /// Do not consider UDP devices as a potential connection targets
    pub fn no_udp(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::NoUdp);
        f
    }

    /// Select IPC node by path
    pub fn ipc_path(self, path: String) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::IpcPath { path });
        f
    }

    /// Select IPC node by USB device it multiplexes
    pub fn usb_over_ipc(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::UsbOverIpc);
        f
    }

    /// Consider IPC devices as a potential connection targets
    pub fn ipc(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::Ipc);
        f
    }

    /// Do not consider IPC devices as a potential connection targets
    pub fn no_ipc(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::NoIpc);
        f
    }

    /// Select in-process node by path
    pub fn in_process_path(self, path: String) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::InProcessPath { path });
        f
    }

    /// Do not consider IPC devices as a potential connection targets
    pub fn no_in_process(self) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::NoInProcess);
        f
    }
    /// Connect over RTT through a debug probe (requires the `rtt` feature), `target` is the probe-rs chip name
    /// (e.g., `RP2040`, `STM32G474RETx`), `speed_hz` is the SWD / JTAG clock (probe default if None).
    ///
    /// Filters _after_ this call select the probe: [usb_vid_pid](Self::usb_vid_pid), [serial_eq](Self::serial_eq)
    /// and [serial_contains](Self::serial_contains), not needed if there is only one. Filters _before_ it describe
    /// the device and are not used to select the probe (the device is not known before connecting), so that
    /// `MyDevice::default_config().rtt(..)` works. WireWeaver USB is not tried when RTT is selected.
    pub fn rtt(self, target: String, speed_hz: Option<u32>) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::Rtt {
            target,
            protocol: (),
            speed_hz,
        });
        f
    }

    /// Take the RTT control block address from the firmware's ELF file (the `_SEGGER_RTT` symbol), instead of
    /// scanning all RAM for it, which takes a while on chips with a lot of RAM (~1.5 s for 144 KiB through an
    /// ST-LINK). If the running firmware doesn't match the file, RAM is scanned anyway, with a warning.
    /// A file that can't be read or has no RTT fails the connection.
    pub fn rtt_elf(self, path: impl Into<PathBuf>) -> Self {
        let mut f = self;
        f.pieces
            .push(ConfigPiece::RttControlBlock(RttControlBlock::Elf(
                path.into(),
            )));
        f
    }

    /// Same as [rtt_elf](Self::rtt_elf), with the RTT control block address known already.
    pub fn rtt_control_block_at(self, address: u64) -> Self {
        let mut f = self;
        f.pieces
            .push(ConfigPiece::RttControlBlock(RttControlBlock::Address(
                address,
            )));
        f
    }

    /// CommandSender queue size, limits the amount of simultaneous requests in-flight.
    /// Default is 8192, using more than 65_534 will lead to blocking if reached.
    pub fn cmd_queue_size(self, size: usize) -> Self {
        let mut f = self;
        f.cmd_queue_size = Some(size);
        f
    }

    /// Largest request seq number to use, default is [DEFAULT_MAX_SEQ](crate::DEFAULT_MAX_SEQ) (3 bytes on the wire).
    /// Seq numbers up to 127 take 1 byte and are used first, bigger ones only while all of these are in flight, so
    /// this limits the number of requests waiting for an answer at the same time.
    pub fn max_seq(self, max_seq: SeqTy) -> Self {
        let mut c = self;
        c.max_seq = Some(max_seq);
        c
    }

    pub fn introspect_client(self, ww_self_bytes: &[u8], api_hash: ApiHashPairOwned) -> Self {
        let mut c = self;
        c.introspect_client = Some((ww_self_bytes.to_vec(), api_hash));
        c
    }

    /// API crate name and version this client was generated from, sent to the device during link setup.
    /// A device implementing an incompatible version refuses the connection.
    /// Generated clients set it automatically, if not set, the client is treated as dynamic
    /// (working with the API via introspection) and no version check is performed.
    pub fn client_version(self, version: FullVersionOwned) -> Self {
        let mut c = self;
        c.client_version = Some(Box::new(version));
        c
    }

    pub fn default_timeout(self, timeout: Duration) -> Self {
        let mut c = self;
        c.default_timeout = Some(timeout);
        c
    }

    /// Filter out a device with the specified serial number. Ignoring case.
    pub fn serial_eq(self, serial: String) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::SerialEq { serial });
        f
    }

    /// Filter out a device whose serial number contains the substring. Ignoring case.
    pub fn serial_contains(self, substring: String) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::SerialContains { substring });
        f
    }

    /// Filter out a device with the specified user label. Ignoring case.
    /// User labels are set by the firmware (see [USB transport](https://ww.vhrd.tech/transport/usb/)), `ww list` shows them.
    pub fn user_label_eq(self, user_label: String) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::UserLabelEq { user_label });
        f
    }

    /// Filter out a device whose manufacturer string contains the substring. Ignoring case.
    pub fn manufacturer_contains(self, substring: String) -> Self {
        let mut f = self;
        f.pieces
            .push(ConfigPiece::ManufacturerContains { substring });
        f
    }

    /// Filter out a device whose product string contains the substring. Ignoring case.
    pub fn product_contains(self, substring: String) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::ProductContains { substring });
        f
    }

    /// Filter out a device that implements specified WireWeaver API.
    pub fn implements_api(self, api_gid: String, version_req: VersionReq) -> Self {
        let mut f = self;
        f.pieces.push(ConfigPiece::ImplementsApi {
            api_gid,
            version_req,
        });
        f
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
    /// Human-readable device filters (serial, label, product, API, VID:PID, etc.), for error messages.
    pub(crate) fn describe_filters(&self) -> Vec<String> {
        self.pieces
            .iter()
            .filter_map(|p| {
                Some(match p {
                    ConfigPiece::UsbVidPid { vid, pid } => {
                        format!("USB VID:PID = {vid:04x}:{pid:04x}")
                    }
                    ConfigPiece::UsbPath { bus_id, port_chain } => {
                        let ports: Vec<String> = port_chain.iter().map(|p| p.to_string()).collect();
                        format!("USB location = {bus_id}-{}", ports.join("."))
                    }
                    ConfigPiece::SerialEq { serial } => format!("serial = \"{serial}\""),
                    ConfigPiece::SerialContains { substring } => {
                        format!("serial contains \"{substring}\"")
                    }
                    ConfigPiece::UserLabelEq { user_label } => format!("label = \"{user_label}\""),
                    ConfigPiece::ManufacturerContains { substring } => {
                        format!("manufacturer contains \"{substring}\"")
                    }
                    ConfigPiece::ProductContains { substring } => {
                        format!("product contains \"{substring}\"")
                    }
                    ConfigPiece::ImplementsApi {
                        api_gid,
                        version_req,
                    } => format!("API = {api_gid} {version_req}"),
                    _ => return None,
                })
            })
            .collect()
    }

    /// USB filters select a probe when RTT is used, not a device to connect to over USB
    pub(crate) fn is_usb(&self) -> bool {
        !self.is_rtt()
            && self.is_opted_in(
                &[
                    ConfigPieceDiscriminants::UsbPath,
                    ConfigPieceDiscriminants::UsbVidPid,
                    ConfigPieceDiscriminants::Usb,
                ],
                ConfigPieceDiscriminants::NoWwUsb,
            )
    }

    pub(crate) fn is_rtt(&self) -> bool {
        self.pieces
            .iter()
            .any(|p| matches!(p, ConfigPiece::Rtt { .. }))
    }

    pub(crate) fn is_websocket(&self) -> bool {
        self.is_opted_in(
            &[
                ConfigPieceDiscriminants::WebSocketAddr,
                ConfigPieceDiscriminants::WebSocket,
            ],
            ConfigPieceDiscriminants::NoWebSocket,
        )
    }

    pub(crate) fn is_udp(&self) -> bool {
        self.is_opted_in(
            &[
                ConfigPieceDiscriminants::UdpAddr,
                ConfigPieceDiscriminants::Udp,
            ],
            ConfigPieceDiscriminants::NoUdp,
        )
    }

    pub(crate) fn is_ipc(&self) -> bool {
        self.is_opted_in(
            &[
                ConfigPieceDiscriminants::IpcPath,
                ConfigPieceDiscriminants::UsbOverIpc,
                ConfigPieceDiscriminants::Ipc,
            ],
            ConfigPieceDiscriminants::NoIpc,
        )
    }

    pub(crate) fn is_in_process(&self) -> bool {
        self.is_opted_in(
            &[ConfigPieceDiscriminants::InProcessPath],
            ConfigPieceDiscriminants::NoInProcess,
        )
    }

    pub(crate) fn interfaces(&self) -> Vec<InterfaceKind> {
        let mut interfaces = vec![];
        if self.is_usb() {
            interfaces.push(InterfaceKind::Usb);
        }
        if self.is_websocket() {
            interfaces.push(InterfaceKind::WebSocket);
        }
        if self.is_udp() {
            interfaces.push(InterfaceKind::Udp);
        }
        if self.is_ipc() {
            interfaces.push(InterfaceKind::Ipc);
        }
        if self.is_in_process() {
            interfaces.push(InterfaceKind::InProcess);
        }
        if self.is_rtt() {
            interfaces.push(InterfaceKind::Rtt);
        }
        // TODO: sort by priority
        interfaces
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
    fn rtt() {
        // device driver config (USB VID:PID of the device) re-used over RTT: USB is not tried
        let f = ClientConfig::new()
            .usb_vid_pid(0xc0de, 0xcafe)
            .rtt("RP2040".into(), None)
            .usb_vid_pid(0x1366, 0x0105)
            .validate()
            .unwrap();
        assert!(!f.is_usb());
        assert_eq!(f.interfaces(), vec![InterfaceKind::Rtt]);
    }

    #[test]
    fn usb() {
        let f = ClientConfig::new().validate().unwrap();
        assert!(!f.is_usb()); // unless opted-in, interface is not considered

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
        assert!(f.is_usb());

        let f = ClientConfig::new().usb().validate().unwrap();
        assert_eq!(f.pieces, vec![ConfigPiece::Usb]);
        assert!(f.is_usb());

        let f = ClientConfig::new().usb().no_ww_usb().validate().unwrap();
        assert_eq!(f.pieces, vec![ConfigPiece::Usb, ConfigPiece::NoWwUsb]);
        assert!(!f.is_usb());

        let f = ClientConfig::new()
            .usb()
            .no_ww_usb()
            .usb()
            .validate()
            .unwrap();
        assert_eq!(
            f.pieces,
            vec![ConfigPiece::Usb, ConfigPiece::NoWwUsb, ConfigPiece::Usb]
        );
        assert!(f.is_usb());
    }

    fn device(serial: &str, label: &str, api: Option<(&str, &str)>) -> DeviceInfo {
        DeviceInfo {
            location: "usb 1-2 c0de:cafe".into(),
            manufacturer: "vhrd.tech".into(),
            product: "Blinky board".into(),
            serials: vec![serial.into()],
            user_label: label.into(),
            api: api.map(|(gid, version)| crate::ApiInfo {
                gid: gid.into(),
                version: semver::Version::parse(version).unwrap(),
                signature: ww_version::ApiHashOwned { hash: vec![] },
            }),
            usb: Some(crate::UsbLocation {
                bus_id: "1".into(),
                port_chain: vec![2],
                vid: 0xc0de,
                pid: 0xcafe,
            }),
        }
    }

    #[test]
    fn matching_no_filters() {
        assert!(ClientConfig::new().usb().matches(&device("A1", "", None)));
    }

    #[test]
    fn matching_different_kinds_all_required() {
        let d = device("ABC123", "left", Some(("blinky_api", "0.1.2")));
        let c = ClientConfig::new()
            .implements_api("blinky_api".into(), VersionReq::parse("^0.1").unwrap())
            .serial_contains("c12".into());
        assert!(c.matches(&d));
        assert!(!c.clone().user_label_eq("right".into()).matches(&d));
        assert!(c.clone().user_label_eq("LEFT".into()).matches(&d));
        assert!(!c.clone().usb_vid_pid(0x1234, 0xcafe).matches(&d));
        assert!(c.clone().usb_port_chain("1".into(), vec![2]).matches(&d));
        let wrong_version = ClientConfig::new()
            .implements_api("blinky_api".into(), VersionReq::parse("^0.2").unwrap());
        assert!(!wrong_version.matches(&d));
    }

    #[test]
    fn matching_same_kind_any() {
        let d = device("ABC123", "", None);
        let c = ClientConfig::new()
            .serial_eq("xyz".into())
            .serial_eq("abc123".into());
        assert!(c.matches(&d));
        assert!(!ClientConfig::new().serial_eq("abc".into()).matches(&d));
        assert!(
            ClientConfig::new()
                .product_contains("BLINKY".into())
                .matches(&d)
        );
        assert!(
            !ClientConfig::new()
                .manufacturer_contains("acme".into())
                .matches(&d)
        );
    }
}
