use crate::{MIN_MESSAGE_SIZE, ReceiverStats, SenderStats};
use shrink_wrap::BufWriter;
use strum_macros::FromRepr;
use wire_weaver::{ProtocolInfo, ww_repr};

// Packs and unpacks messages to/from one or more USB packets.
// Message size is only limited by remote end buffer size (and u32::MAX, which is unlikely to be the case).
//
// Packets are not sent immediately to collect more messages into one packet and lower overhead.
//
// To ensure backward and forward format compatibility, there is a link setup phase, during which user protocol,
// this link version and buffer sizes are exchanged.
pub struct WireWeaverUsbLink<'i, T, R> {
    // Link info and status
    pub(crate) user_protocol: ProtocolInfo,
    pub(crate) client_server_protocol: ProtocolInfo,
    pub(crate) remote_protocol: Option<ProtocolInfo>,
    pub(crate) remote_max_message_size: u32,

    // Sender
    pub(crate) tx: T,
    pub(crate) tx_writer: BufWriter<'i>,
    pub(crate) tx_stats: SenderStats,

    // Receiver
    pub(crate) rx: R,
    pub(crate) rx_packet_buf: &'i mut [u8],
    pub(crate) rx_start_pos: usize,
    pub(crate) rx_left_bytes: usize,
    pub(crate) rx_stats: ReceiverStats,
    pub(crate) rx_in_fragmented_message: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<T, R> {
    InternalBufOverflow,
    ProtocolsVersionMismatch,
    LinkVersionMismatch,
    Disconnected,

    SourceError(R),
    ReceivedEmptyPacket,

    SinkError(T),
    MessageTooBig,
}

/// Interface used by [MessageSender](crate::MessageSender) to send packets to USB bus.
/// Additionally, if device feature is enabled, [LinkMgmtCmd] are passed from MessageReceiver to MessageSender.
pub trait PacketSink {
    type Error;
    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

/// Interface used by [MessageReceiver](crate::MessageReceiver) to receive packets from USB bus.
pub trait PacketSource {
    type Error;
    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error>;

    #[cfg(feature = "device")]
    async fn wait_usb_connection(&mut self);
}

impl<'i, T: PacketSink, R: PacketSource> WireWeaverUsbLink<'i, T, R> {
    pub fn new(
        client_server_protocol: ProtocolInfo,
        user_protocol: ProtocolInfo,
        tx: T,
        tx_packet_buf: &'i mut [u8],
        rx: R,
        rx_packet_buf: &'i mut [u8],
    ) -> Self {
        let tx_writer = BufWriter::new(tx_packet_buf);

        #[cfg(test)]
        let remote_protocol = Some(ProtocolInfo {
            protocol_id: 0,
            major_version: 0,
            minor_version: 0,
        });
        #[cfg(not(test))]
        let remote_protocol = None;

        WireWeaverUsbLink {
            client_server_protocol,
            user_protocol,
            remote_max_message_size: MIN_MESSAGE_SIZE as u32,
            remote_protocol,

            tx,
            tx_writer,
            tx_stats: Default::default(),

            rx,
            rx_packet_buf,
            rx_start_pos: 0,
            rx_left_bytes: 0,
            rx_stats: Default::default(),
            rx_in_fragmented_message: false,
        }
    }

    #[cfg(feature = "device")]
    /// Device only function. Waits for physical USB cable connection and interface enable.
    pub async fn wait_usb_connection(&mut self) {
        self.rx.wait_usb_connection().await;
    }

    /// Marks link as not connected, but does not send anything to the host.
    pub fn silent_disconnect(&mut self) {
        self.remote_protocol = None;
        self.remote_max_message_size = MIN_MESSAGE_SIZE as u32;
    }
}

pub(crate) const VERSIONS_PAYLOAD_LEN: usize = 4 + 1 + ProtocolInfo::size_bytes() * 2;

#[repr(u8)]
#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
pub(crate) enum Op {
    Nop = 0,

    /// 0x1l, 0xll, `data[0..len]` in first packet
    MessageStart = 1,
    /// 0x2l, 0xll, `data[prev..prev+len]` at the start of next packet
    MessageContinue = 2,
    /// 0x3l, 0xll, `data[prev..prev+len]`, CRC (2 bytes) at the start of next packet
    MessageEnd = 3,
    /// 0x4l, 0xll, `data[0..len]` in one packet.
    MessageStartEnd = 4,

    /// Sent from host to device to get link version, WireWeaver client server protocol version and
    /// user protocol global ID and version
    GetDeviceInfo = 5,
    /// Sent from device to host in response to GetDeviceInfo
    DeviceInfo = 6,

    /// Sent from host to device with its link, client server and user versions
    LinkSetup = 7,
    /// Sent from device to host to let it know that it received LinkSetup.
    /// Guard against host starting to send before device received LinkSetup to avoid losing messages.
    LinkSetupResult = 8,

    Ping = 9,

    /// Sent from host to device to let it know that driver or application is stopping.
    /// Sent from device to host to let it know that it is rebooting, e.g. to perform fw update.
    Disconnect = 10,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DisconnectReason {
    ApplicationCrash,
    RequestByUser,
    Other,
    Unknown,
}
