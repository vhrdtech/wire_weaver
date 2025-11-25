use crate::{MIN_MESSAGE_SIZE, ReceiverStats, SenderStats};
use shrink_wrap::stack_vec::StackVec;
use strum_macros::FromRepr;
use wire_weaver::prelude::*;
use wire_weaver::ww_version::CompactVersion;

// Packs and unpacks messages to/from one or more USB packets.
// Message size is only limited by remote end buffer size (and u32::MAX, which is unlikely to be the case).
//
// Packets are not sent immediately to collect more messages into one packet and lower overhead.
//
// To ensure backward and forward format compatibility, there is a link setup phase, during which user protocol,
// this link version and buffer sizes are exchanged.
pub struct WireWeaverUsbLink<'i, T, R> {
    // Link info and status
    /// User-defined data types and API, also indirectly points to ww_client_server version
    pub(crate) user_protocol: FullVersion<'static>,

    /// Remote user protocol version
    pub(crate) remote_protocol: StackVec<32, FullVersion<'static>>,

    pub(crate) remote_max_message_size: u32,

    // Sender
    pub(crate) tx: T,
    pub(crate) tx_writer: BufWriter<'i>,
    pub(crate) tx_stats: SenderStats,

    // Receiver
    pub(crate) rx: R,
    /// Used to hold up to one USB packet (64..=1024B)
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
        user_protocol: FullVersion<'static>,
        tx: T,
        tx_packet_buf: &'i mut [u8],
        rx: R,
        rx_packet_buf: &'i mut [u8],
    ) -> Self {
        let tx_writer = BufWriter::new(tx_packet_buf);

        #[cfg(test)]
        let remote_protocol =
            StackVec::some(FullVersion::new("test", ww_version::Version::new(0, 0, 0)))
                .expect("FullVersion in StackVec in test");
        #[cfg(not(test))]
        let remote_protocol = StackVec::none();

        WireWeaverUsbLink {
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

    /// Marks link as not connected, but does not send anything to the other party.
    pub fn silent_disconnect(&mut self) {
        self.remote_protocol.clear();
        self.remote_max_message_size = MIN_MESSAGE_SIZE as u32;
    }

    #[cfg(feature = "device")]
    pub(crate) fn is_protocol_compatible(&self) -> bool {
        self.remote_protocol.is_some()
    }

    pub(crate) fn is_link_up(&self) -> bool {
        self.remote_protocol.is_some()
    }
}

#[repr(u8)]
#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
pub(crate) enum Op {
    /// See [send_nop()](WireWeaverUsbLink::send_nop) docs for explanation as to why this is needed.
    Nop = 0,

    /// Sent from host to device to get device info.
    GetDeviceInfo = 1,
    /// Sent from device to host in response to GetDeviceInfo.
    /// Sent in one packet with [DeviceInfo](DeviceInfo) struct following, since link is not up yet and ShrinkWrap uses buffer till its end.
    DeviceInfo = 2,

    /// Sent from host to device with its link, client server and user versions.
    /// Sent in one packet with [LinkSetup](LinkSetup) struct following, since link is not up yet and ShrinkWrap uses buffer till its end.
    LinkSetup = 3,
    /// Sent from device to host to let it know that it received LinkSetup and that protocol version is compatibly.
    /// Otherwise, Disconnect with DisconnectReason::IncompatibleVersion is sent.
    /// Guard against host starting to send before device received LinkSetup to avoid losing messages.
    LinkReady = 4,

    /// 0x1l, 0xll, `data[0..len]` in first packet, note that len is not full message length, but only the length of the piece in current packet
    MessageStart = 5,
    /// 0x2l, 0xll, `data[prev..prev+len]` at the start of next packet
    MessageContinue = 6,
    /// 0x3l, 0xll, `data[prev..prev+len]`, CRC (2 bytes) at the start of next packet
    MessageEnd = 7,
    /// 0x4l, 0xll, `data[0..len]` in one packet.
    MessageStartEnd = 8,

    /// Sent periodically when there are no data messages from both host and device sides
    Ping = 9,

    /// Sent from host to device, if requested by user
    GetStats = 10,
    /// Sent in response to GetStats from device side
    Stats = 11,

    /// Sent from host to device to let it know that driver or application is stopping.
    /// Sent from device to host to let it know that it is rebooting, e.g. to perform fw update.
    Disconnect = 15,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive_shrink_wrap]
#[ww_repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DisconnectReason {
    ApplicationCrash,
    RequestByUser,
    IncompatibleVersion,
    Other(u8),
    Unknown,
}

#[derive_shrink_wrap]
struct DeviceInfo<'i> {
    /// This crate major and minor version on the device side
    dev_link_version: CompactVersion,
    /// User API and data types version on the device side
    dev_user_version: FullVersion<'i>,
    /// Maximum length message that device can process
    dev_max_message_len: u32,
}

#[derive_shrink_wrap]
struct LinkSetup<'i> {
    /// User API and data types version on the host side
    host_user_version: FullVersion<'i>,
    /// Maximum length message that host can process
    host_max_message_len: u32,
}
