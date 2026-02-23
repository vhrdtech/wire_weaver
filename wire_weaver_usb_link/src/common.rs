use crate::{ReceiverStats, SenderStats, MIN_MESSAGE_SIZE};
use shrink_wrap::ww_repr;
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
    /// User-defined data types and API, also indirectly points to `ww_client_server` version
    #[cfg(feature = "device")]
    pub(crate) user_api_version: FullVersion<'static>,
    #[cfg(feature = "device")]
    pub(crate) user_api_signature: &'static [u8],
    #[cfg(feature = "device")]
    pub(crate) api_model_version: wire_weaver::ww_version::CompactVersion,
    #[cfg(feature = "host")]
    pub(crate) user_api_version: ww_version::FullVersionOwned,

    // /// Remote user protocol version
    // pub(crate) remote_protocol: StackVec<32, FullVersion<'static>>,
    pub(crate) is_link_up: bool,

    pub(crate) remote_max_message_size: u32,

    // Sender
    pub(crate) tx: T,
    pub(crate) tx_writer: BufWriter<'i>,
    pub(crate) tx_stats: SenderStats,

    // Receiver
    pub(crate) rx: R,
    /// Used to hold up to one USB packet (64 - 1024B)
    pub(crate) rx_packet_buf: &'i mut [u8],
    pub(crate) rx_start_pos: usize,
    pub(crate) rx_left_bytes: usize,
    pub(crate) rx_stats: ReceiverStats,
    pub(crate) rx_in_fragmented_message: bool,
    pub(crate) staging_idx: usize,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<T, R> {
    InternalBufOverflow,
    UnexpectedOp(Op),
    ProtocolsVersionMismatch,
    LinkVersionMismatch,
    Disconnected,

    SourceError(R),
    ReceivedEmptyPacket,

    SinkError(T),
    MessageTooBig,
}

/// Interface used by [MessageSender](crate::MessageSender) to send packets to USB bus.
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
        #[cfg(feature = "device")] user_api_version: FullVersion<'static>,
        #[cfg(feature = "device")] user_api_signature: &'static [u8],
        #[cfg(feature = "device")] api_model_version: wire_weaver::ww_version::CompactVersion,
        #[cfg(feature = "host")] user_api_version: ww_version::FullVersionOwned,
        tx: T,
        tx_packet_buf: &'i mut [u8],
        rx: R,
        rx_packet_buf: &'i mut [u8],
    ) -> Self {
        let tx_writer = BufWriter::new(tx_packet_buf);

        // #[cfg(test)]
        // let remote_protocol =
        //     StackVec::some(FullVersion::new("test", ww_version::Version::new(0, 0, 0)))
        //         .expect("FullVersion in StackVec in test");
        // #[cfg(not(test))]
        // let remote_protocol = StackVec::none();
        #[cfg(test)]
        let is_link_up = true;
        #[cfg(not(test))]
        let is_link_up = false;

        WireWeaverUsbLink {
            user_api_version,
            #[cfg(feature = "device")]
            api_model_version,
            #[cfg(feature = "device")]
            user_api_signature,
            remote_max_message_size: MIN_MESSAGE_SIZE as u32,
            // remote_protocol,
            is_link_up,

            tx,
            tx_writer,
            tx_stats: Default::default(),

            rx,
            rx_packet_buf,
            rx_start_pos: 0,
            rx_left_bytes: 0,
            rx_stats: Default::default(),
            rx_in_fragmented_message: false,
            staging_idx: 0,
        }
    }

    #[cfg(feature = "device")]
    /// Device only function. Waits for physical USB cable connection and interface enable.
    pub async fn wait_usb_connection(&mut self) {
        self.rx.wait_usb_connection().await;
    }

    /// Marks link as not connected, but does not send anything to the other party.
    pub fn silent_disconnect(&mut self) {
        // self.remote_protocol.clear();
        self.is_link_up = false;
        self.remote_max_message_size = MIN_MESSAGE_SIZE as u32;
    }

    #[cfg(feature = "device")]
    pub(crate) fn is_protocol_compatible(&self) -> bool {
        self.is_link_up
    }

    pub(crate) fn is_link_up(&self) -> bool {
        self.is_link_up
    }

    // /// Get a mutable reference to tx
    // pub fn tx_mut(&mut self) -> &mut T {
    //     &mut self.tx
    // }
    //
    // /// Get a mutable reference to rx
    // pub fn rx_mut(&mut self) -> &mut R {
    //     &mut self.rx
    // }

    #[cfg(feature = "device")]
    pub fn user_protocol(&self) -> FullVersion<'static> {
        self.user_api_version.clone()
    }

    #[cfg(feature = "host")]
    pub fn user_protocol(&self) -> ww_version::FullVersionOwned {
        self.user_api_version.clone()
    }
}

//noinspection GrazieInspection
#[repr(u8)]
#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Op {
    /// See [send_nop()](WireWeaverUsbLink::send_nop) docs for explanation as to why this is needed.
    Nop = 0,

    /// Sent from host to device to get device info.
    GetDeviceInfo = 1,
    /// Sent from device to host in response to GetDeviceInfo.
    /// Sent in one packet with [DeviceInfo](DeviceInfo) struct following, since link is not up yet and ShrinkWrap uses buffer till its end.
    DeviceInfo = 2,

    /// Sent from host to device with its link, client server, and user versions.
    /// Sent in one packet with [LinkSetup](LinkSetup) struct following, since link is not up yet and ShrinkWrap uses buffer till its end.
    LinkSetup = 3,
    /// Sent from device to host to let it know that it received LinkSetup and that protocol version is compatibly.
    /// Otherwise, Disconnect with DisconnectReason::IncompatibleVersion is sent.
    /// Guard against host starting to send before device received LinkSetup to avoid losing messages.
    LinkReady = 4,

    /// 0x5l, 0xll, `data[0..len]` in first packet, note that len is not full message length, but only the length of the the piece in current packet
    MessageStart = 5,
    /// 0x6l, 0xll, `data[prev..prev+len]` at the start of next packet
    MessageContinue = 6,
    /// 0x7l, 0xll, `data[prev..prev+len]`, CRC (2 bytes) at the start of next packet
    MessageEnd = 7,
    /// 0x8l, 0xll, `data[0..len]` in one packet.
    MessageStartEnd = 8,

    /// Sent periodically when there are no data messages from both host and device sides
    Ping = 9,

    /// Sent from host to device, if requested by user
    GetStats = 10,
    /// Sent in response to GetStats from device side
    Stats = 11,

    /// Used to test hardware and software stack by sending lots of data back and forth.
    /// This command is followed by two u32's in LE and then test data till the end of a packet.
    /// | repeat | seq | data ... |
    ///
    /// repeat:
    /// * 0 - only count incoming packets, do not answer (used to measure host->device speed)
    /// * 1 and up - send one or more copies back (1 used to test the link integrity, more than 1 to test device-> host speed).
    Loopback = 12,

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
    /// This crate version on the device side
    dev_link_version: CompactVersion,
    /// E.g., ww_client_server version on the device side
    api_model_version: CompactVersion,
    /// User API and data types version on the device side
    user_api_version: FullVersion<'i>,
    /// First 8 bytes for SHA256 of ww_self bytes without doc comments
    user_api_signature: RefVec<'i, u8>,
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
