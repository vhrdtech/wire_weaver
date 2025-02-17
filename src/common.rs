use crate::{ReceiverStats, SenderStats, MIN_MESSAGE_SIZE};
use shrink_wrap::{BufReader, BufWriter};
use strum_macros::FromRepr;
use wire_weaver_derive::ww_repr;

// Packs and unpacks messages to/from one or more USB packets.
// Message size is only limited by remote end buffer size (and u32::MAX, which is unlikely to be the case).
//
// Packets are not sent immediately to collect more messages into one packet and lower overhead.
//
// To ensure backward and forward format compatibility, there is a link setup phase, during which user protocol,
// this link version and buffer sizes are exchanged.
pub struct WireWeaverUsbLink<'i, T, R> {
    // Link info and status
    pub(crate) protocol: ProtocolInfo,
    pub(crate) remote_max_message_size: u32,
    pub(crate) remote_protocol: Option<ProtocolInfo>,

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

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<T, R> {
    InternalBufOverflow,
    ProtocolsVersionMismatch,
    LinkVersionMismatch,
    Disconnected,

    SourceError(R),
    ReceivedEmptyPacket,

    SinkError(T),
    EmptyMessage,
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

/// User protocol ID and version. Only major and minor numbers are used and checked.
/// Protocols are compatible if IDs are equal and if major versions matches for major >= 1.
/// So all 1.x and 1.y series are considered compatible, so that older firmwares can talk to newer
/// host software and older host software can talk to newer firmwares.
///
/// If major == 0, then only minor versions are compared. I.e. 0.1 and 0.2 are incompatible and can
/// be used during development.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ProtocolInfo {
    pub protocol_id: u32,
    pub major_version: u8,
    pub minor_version: u8,
}

impl<'i, T: PacketSink, R: PacketSource> WireWeaverUsbLink<'i, T, R> {
    pub fn new(
        protocol: ProtocolInfo,
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
            protocol,
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

#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
pub(crate) enum Op {
    NoOp = 0,

    /// 0x1l, 0xll, `data[0..len]` in first packet
    MessageStart = 1,
    /// 0x2l, 0xll, `data[prev..prev+len]` at the start of next packet
    MessageContinue = 2,
    /// 0x3l, 0xll, `data[prev..prev+len]`, CRC (2 bytes) at the start of next packet
    MessageEnd = 3,
    /// 0x4l, 0xll, `data[0..len]` in one packet.
    MessageStartEnd = 4,

    GetVersions = 5,
    LinkSetup = 6,

    Ping = 7,

    Disconnect = 8,
}

impl ProtocolInfo {
    pub(crate) const fn size_bytes() -> usize {
        6
    }

    pub(crate) fn write(&self, wr: &mut BufWriter) -> Result<(), shrink_wrap::Error> {
        wr.write_u32(self.protocol_id)?;
        wr.write_u8(self.major_version)?;
        wr.write_u8(self.minor_version)?;
        Ok(())
    }

    pub(crate) fn read(rd: &mut BufReader) -> Result<ProtocolInfo, shrink_wrap::Error> {
        Ok(ProtocolInfo {
            protocol_id: rd.read_u32()?,
            major_version: rd.read_u8()?,
            minor_version: rd.read_u8()?,
        })
    }

    pub fn is_compatible(&self, other: &ProtocolInfo) -> bool {
        if self.protocol_id != other.protocol_id {
            false
        } else {
            if self.major_version == 0 && other.major_version == 0 {
                self.minor_version == other.minor_version
            } else {
                // not comparing minor versions, protocols are supposed to be backwards and forwards compatible after 1.0
                self.major_version == other.major_version
            }
        }
    }
}
