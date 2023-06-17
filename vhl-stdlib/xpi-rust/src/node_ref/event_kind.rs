use crate::error::XpiError;
use crate::event_kind::XpiGenericEventKind;
use crate::xwfd::{Rate, ResourceInfo};
use core::fmt::{Display, Formatter};
use vhl_stdlib::serdes::vlu4::Vlu4Vec;
use vhl_stdlib::serdes::NibbleBuf;

/// Highly space efficient xPI EventKind data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericEventKind](crate::event_kind::XpiGenericEventKind) for detailed information.
pub type EventKind<'ev> = XpiGenericEventKind<
    // &'ev [u8], // SL
    Vlu4Vec<'ev, NibbleBuf<'ev>>,                      // VSL
    Vlu4Vec<'ev, Rate>,                                // VR
    Vlu4Vec<'ev, Result<NibbleBuf<'ev>, XpiError>>,    // VRSL
    Vlu4Vec<'ev, Result<(), XpiError>>,                // VRU
    Vlu4Vec<'ev, Result<ResourceInfo<'ev>, XpiError>>, // VRI
    (),                                                // Node info
    (),                                                // Heartbeat info
>;

impl<'i> EventKind<'i> {
    pub fn des_vlu4_with_discriminant<'di>(
        discriminant: u8,
        nrd: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, XpiError> {
        match discriminant {
            0 => Ok(EventKind::Call {
                args_set: nrd.des_vlu4()?,
            }),
            1 => Ok(EventKind::Read),
            2 => Ok(EventKind::Write {
                values: nrd.des_vlu4()?,
            }),
            3 => Ok(EventKind::OpenStreams),
            4 => Ok(EventKind::CloseStreams),
            5 => Ok(EventKind::Subscribe {
                rates: nrd.des_vlu4()?,
            }),
            6 => Ok(EventKind::Unsubscribe),
            7 => Ok(EventKind::Borrow),
            8 => Ok(EventKind::Release),
            9 => Ok(EventKind::Introspect),

            16 => Ok(EventKind::CallResults(nrd.des_vlu4()?)),
            17 => Ok(EventKind::ReadResults(nrd.des_vlu4()?)),
            18 => Ok(EventKind::WriteResults(nrd.des_vlu4()?)),
            19 => Ok(EventKind::OpenStreamsResults(nrd.des_vlu4()?)),
            20 => Ok(EventKind::CloseStreamsResults(nrd.des_vlu4()?)),
            21 => Ok(EventKind::SubscribeResults(nrd.des_vlu4()?)),
            22 => Ok(EventKind::UnsubscribeResults(nrd.des_vlu4()?)),
            23 => Ok(EventKind::BorrowResults(nrd.des_vlu4()?)),
            24 => Ok(EventKind::ReleaseResults(nrd.des_vlu4()?)),
            25 => Ok(EventKind::IntrospectResults(nrd.des_vlu4()?)),
            31 => Ok(EventKind::RateChangeResults(nrd.des_vlu4()?)),

            32 => Ok(EventKind::StreamUpdates(nrd.des_vlu4()?)),
            33 => Ok(EventKind::DiscoverNodes),
            34 => Ok(EventKind::NodeInfo(nrd.des_vlu4()?)),
            35 => Ok(EventKind::Heartbeat(())),

            48 => Ok(EventKind::Forward),

            _ => Err(XpiError::ReservedDiscard),
        }
    }
}

impl<'i> Display for EventKind<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            EventKind::Call { args_set } => write!(f, "Call({})", args_set),
            EventKind::Read => write!(f, "Read"),
            EventKind::Write { values } => write!(f, "Write({}", values),
            EventKind::OpenStreams => write!(f, "OpenStreams"),
            EventKind::CloseStreams => write!(f, "CloseStreams"),
            EventKind::Subscribe { rates } => write!(f, "Subscribe({:?})", rates),
            EventKind::Unsubscribe => write!(f, "Unsubscribe"),
            EventKind::Borrow => write!(f, "Borrow"),
            EventKind::Release => write!(f, "Release"),
            EventKind::Introspect => write!(f, "Introspect"),
            EventKind::CallResults(results) => write!(f, "CallResults({:?})", results),
            EventKind::ReadResults(values) => write!(f, "ReadResults({:?})", values),
            EventKind::WriteResults(values) => write!(f, "WriteResults({:?})", values),
            EventKind::OpenStreamsResults(results) => {
                write!(f, "OpenStreamsResults({:?})", results)
            }
            EventKind::CloseStreamsResults(results) => {
                write!(f, "CloseStreamsResults({:?})", results)
            }
            EventKind::SubscribeResults(results) => write!(f, "SubscribeResults({:?})", results),
            EventKind::RateChangeResults(results) => write!(f, "RateChangeResults({:?})", results),
            EventKind::UnsubscribeResults(results) => {
                write!(f, "UnsubscribeResults({:?})", results)
            }
            EventKind::BorrowResults(results) => write!(f, "BorrowResults({:?})", results),
            EventKind::ReleaseResults(results) => write!(f, "ReleaseResults({:?})", results),
            EventKind::IntrospectResults(results) => write!(f, "IntrospectResults({:?})", results),
            EventKind::StreamUpdates(updates) => write!(f, "StreamUpdates({:?})", updates),
            EventKind::DiscoverNodes => write!(f, "DiscoverNodes"),
            EventKind::NodeInfo(node_info) => write!(f, "NodeInfo({:?})", node_info),
            EventKind::Heartbeat(hb_info) => write!(f, "Heartbeat({:?})", hb_info),
            EventKind::Forward => write!(f, "Forward"),
        }
    }
}
