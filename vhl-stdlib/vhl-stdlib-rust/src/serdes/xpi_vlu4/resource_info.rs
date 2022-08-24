use crate::serdes::{DeserializeVlu4, NibbleBuf};
use crate::serdes::vlu4::Vlu4Vec;
use crate::serdes::xpi_vlu4::NodeId;
use crate::serdes::xpi_vlu4::rate::Rate;
use crate::serdes::nibble_buf::Error as NibbleBufError;

#[derive(Copy, Clone, Debug)]
pub enum ResourceInfo<'i> {
    FreeResource,
    BorrowedResource {
        borrowed_by: NodeId
    },
    ClosedStream,
    OpenStream {
        /// As all streams are implicitly wrapped in a Cell<_> in order to use it, node have to
        /// make a borrow first.
        borrowed_by: NodeId,
        /// TODO: Not sure whether multiple stream subscribers is needed, and how to get around Cell in that case
        subscribers: Vlu4Vec<'i, u32>,
        rates: RatesInfo,
    },
    RwStreamProperty {
        subscribers: Vlu4Vec<'i, NodeId>,
        /// Incoming data rates
        rates_in: RatesInfo,
        /// Outgoing data rates
        rates_out: RatesInfo,
    },
    WoStreamProperty {
        subscribers: Vlu4Vec<'i, NodeId>,
        /// Incoming data rates
        rates_in: RatesInfo,
    },
    RoStreamProperty {
        subscribers: Vlu4Vec<'i, NodeId>,
        /// Outgoing data rates
        rates_out: RatesInfo,
    },
    Array {
        size: u32,
    }
}

impl<'i> DeserializeVlu4<'i> for ResourceInfo<'i> {
    type Error = NibbleBufError;

    fn des_vlu4<'di>(_rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        todo!()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct RatesInfo {
    /// Current instant rate of this stream, may differ from requested by congestion control
    pub current_rate: Rate,
    /// Rate that was requested when subscribing
    pub requested_rate: Rate,
    /// Maximum allowed rate of this stream
    pub maximum_rate: Rate,
}
