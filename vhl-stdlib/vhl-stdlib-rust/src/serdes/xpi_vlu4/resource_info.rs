use crate::serdes::vlu4::{Vlu4SliceArray, Vlu4U32Array};
use crate::serdes::xpi_vlu4::NodeId;
use crate::serdes::xpi_vlu4::rate::Rate;

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
        subscribers: Vlu4U32Array<'i>,
        rates: RatesInfo,
    },
    RwStreamProperty {
        subscribers: Vlu4SliceArray<'i>,
        /// Incoming data rates
        rates_in: RatesInfo,
        /// Outgoing data rates
        rates_out: RatesInfo,
    },
    WoStreamProperty {
        subscribers: Vlu4SliceArray<'i>,
        /// Incoming data rates
        rates_in: RatesInfo,
    },
    RoStreamProperty {
        subscribers: Vlu4SliceArray<'i>,
        /// Outgoing data rates
        rates_out: RatesInfo,
    },
    Array {
        size: u32,
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
