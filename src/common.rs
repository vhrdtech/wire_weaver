use shrink_wrap::{BufReader, BufWriter};
use strum_macros::FromRepr;
use wire_weaver_derive::ww_repr;

#[ww_repr(u4)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, FromRepr)]
pub(crate) enum Kind {
    NoOp = 0,

    /// 0x1l, 0xll, `data[0..len]` in first packet
    PacketStart = 1,
    /// 0x2l, 0xll, `data[prev..prev+len]` at the start of next packet
    PacketContinue = 2,
    /// 0x3l, 0xll, `data[prev..prev+len]`, CRC (2 bytes) at the start of next packet
    PacketEnd = 3,
    /// 0x4l, 0xll, `data[0..len]` in one packet.
    PacketStartEnd = 4,

    LinkInfo = 5,

    Ping = 6,

    Disconnect = 7,
}

/// Interface used by [MessageSender](crate::MessageSender) to send packets to USB bus.
/// Additionally, if device feature is enabled, [LinkMgmtCmd] are passed from MessageReceiver to MessageSender.
pub trait PacketSink {
    type Error;
    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error>;

    #[cfg(feature = "device")]
    async fn wait_connection(&mut self);
    #[cfg(feature = "device")]
    async fn rx_from_source(&mut self) -> LinkMgmtCmd;
    #[cfg(feature = "device")]
    fn try_rx_from_source(&mut self) -> Option<LinkMgmtCmd>;
}

/// Interface used by [MessageReceiver](crate::MessageReceiver) to receive packets from USB bus.
pub trait PacketSource {
    type Error;
    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error>;

    #[cfg(feature = "device")]
    async fn wait_connection(&mut self);
    #[cfg(feature = "device")]
    fn send_to_sink(&mut self, msg: LinkMgmtCmd);
}

/// Used to pass information from PacketReceiver to PacketSender to inform it if
/// remote end has disconnected and to pass information for versions checks.
pub enum LinkMgmtCmd {
    Disconnect,
    LinkInfo {
        link_version_matches: bool,
        local_max_packet_size: u32,
        remote_max_message_size: u32,
        remote_protocol: ProtocolInfo,
    },
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
