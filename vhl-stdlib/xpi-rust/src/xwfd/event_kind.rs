use core::fmt::{Display, Formatter};
use crate::error::XpiError;
use crate::event_kind::XpiGenericEventKind;
use crate::xwfd::{Rate, ResourceInfo, XwfdError};
use vhl_stdlib::serdes::vlu4::Vlu4Vec;
use vhl_stdlib::serdes::NibbleBuf;

/// Highly space efficient xPI EventKind data structure supporting zero copy and no_std without alloc
/// even for variable length arrays or strings.
/// See [XpiGenericEventKind](crate::event_kind::XpiGenericEventKind) for detailed information.
pub type EventKind<'ev> = XpiGenericEventKind<
    // &'ev [u8], // SL
    Vlu4Vec<'ev, &'ev [u8]>, // VSL
    Vlu4Vec<'ev, Rate>, // VR
    Vlu4Vec<'ev, Result<&'ev [u8], XpiError>>, // VRSL
    Vlu4Vec<'ev, Result<(), XpiError>>, // VRU
    Vlu4Vec<'ev, Result<ResourceInfo<'ev>, XpiError>>, // VRI
    (), // Node info
    (), // Heartbeat info
>;

impl<'i> EventKind<'i> {
    pub fn des_vlu4_with_discriminant<'di>(
        discriminant: u8,
        nrd: &'di mut NibbleBuf<'i>,
    ) -> Result<Self, XwfdError> {
        match discriminant {
            0 => Ok(EventKind::Call {
                args_set: nrd.des_vlu4()?,
            }),
            1 => unimplemented!(),
            2 => unimplemented!(),
            3 => unimplemented!(),
            4 => unimplemented!(),
            5 => unimplemented!(),
            6 => unimplemented!(),
            7 => unimplemented!(),
            8 => unimplemented!(),
            9 => unimplemented!(),

            16 => Ok(EventKind::CallResults(nrd.des_vlu4()?)),
            17 => unimplemented!(),
            18 => unimplemented!(),
            19 => unimplemented!(),
            20 => unimplemented!(),
            21 => unimplemented!(),
            22 => unimplemented!(),
            23 => unimplemented!(),
            24 => unimplemented!(),
            25 => unimplemented!(),
            31 => unimplemented!(),

            32 => unimplemented!(),
            33 => unimplemented!(),
            34 => unimplemented!(),
            35 => unimplemented!(),

            48 => unimplemented!(),

            _ => Err(XwfdError::ReservedDiscard),
        }
    }
}

impl<'i> Display for EventKind<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            EventKind::Call { args_set } => write!(f, "Call({:?})", args_set),
            EventKind::Read => write!(f, "Read"),
            EventKind::Write { .. } => write!(f, "Write"),
            EventKind::OpenStreams => write!(f, "OpenStreams"),
            EventKind::CloseStreams => write!(f, "CloseStreams"),
            EventKind::Subscribe { .. } => write!(f, "Subscribe"),
            EventKind::Unsubscribe => write!(f, "Unsubscribe"),
            EventKind::Borrow => write!(f, "Borrow"),
            EventKind::Release => write!(f, "Release"),
            EventKind::Introspect => write!(f, "Introspect"),
            EventKind::CallResults(results) => write!(f, "CallResults{:?}", results),
            EventKind::ReadResults(_) => write!(f, "ReadResults"),
            EventKind::WriteResults(_) => write!(f, "WriteResults"),
            EventKind::OpenStreamsResults(_) => write!(f, "OpenStreamsResults"),
            EventKind::CloseStreamsResults(_) => write!(f, "CloseStreamsResults"),
            EventKind::SubscribeResults(_) => write!(f, "SubscribeResults"),
            EventKind::RateChangeResults(_) => write!(f, "RateChangeResults"),
            EventKind::UnsubscribeResults(_) => write!(f, "UnsubscribeResults"),
            EventKind::BorrowResults(_) => write!(f, "BorrowResults"),
            EventKind::ReleaseResults(_) => write!(f, "ReleaseResults"),
            EventKind::IntrospectResults(_) => write!(f, "IntrospectResults"),
            EventKind::StreamUpdates(_) => write!(f, "StreamUpdates"),
            EventKind::DiscoverNodes => write!(f, "DiscoverNodes"),
            EventKind::NodeInfo(_) => write!(f, "NodeInfo"),
            EventKind::Heartbeat(_) => write!(f, "Heartbeat"),
            EventKind::Forward => write!(f, "Forward"),
        }
    }
}