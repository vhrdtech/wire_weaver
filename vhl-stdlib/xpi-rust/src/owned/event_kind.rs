use crate::error::XpiError;
use crate::event_kind::XpiGenericEventKind;
use crate::owned::convert_error::ConvertError;
use crate::owned::{Rate, ResourceInfo};
use crate::xwfd;
use std::fmt::{Display, Formatter};
use vhl_stdlib::serdes::{NibbleBufMut};

pub type EventKind = XpiGenericEventKind<
    Vec<Vec<u8>>,
    Vec<Rate>,
    Vec<Result<Vec<u8>, XpiError>>,
    Vec<Result<(), XpiError>>,
    Vec<Result<(), ResourceInfo>>,
    (),
    u32,
>;

impl EventKind {
    pub fn new_heartbeat(info: u32) -> Self {
        EventKind::Heartbeat(info)
    }

    pub(crate) fn ser_body_xwfd(&self, nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        match &self {
            EventKind::Call { args_set } => {
                nwr.put_vec_with(|vb| {
                    args_set
                        .iter()
                        .try_for_each(|args| vb.put(&args.as_slice()))
                })?;
            }
            // EventKind::Read => {}
            // EventKind::Write { .. } => {}
            // EventKind::OpenStreams => {}
            // EventKind::CloseStreams => {}
            // EventKind::Subscribe { .. } => {}
            // EventKind::Unsubscribe => {}
            // EventKind::Borrow => {}
            // EventKind::Release => {}
            // EventKind::Introspect => {}
            EventKind::CallResults(results) => {
                nwr.put_vec_with(|vb| results.iter().try_for_each(|result| vb.put(result)))?;
            }
            // EventKind::ReadResults(_) => {}
            // EventKind::WriteResults(_) => {}
            // EventKind::OpenStreamsResults(_) => {}
            // EventKind::CloseStreamsResults(_) => {}
            // EventKind::SubscribeResults(_) => {}
            // EventKind::RateChangeResults(_) => {}
            // EventKind::UnsubscribeResults(_) => {}
            // EventKind::BorrowResults(_) => {}
            // EventKind::ReleaseResults(_) => {}
            // EventKind::IntrospectResults(_) => {}
            // EventKind::StreamUpdates(_) => {}
            // EventKind::DiscoverNodes => {}
            // EventKind::NodeInfo(_) => {}
            // EventKind::Heartbeat(_) => {}
            // EventKind::Forward => {}
            _ => unimplemented!(),
        }
        Ok(())
    }
}

impl<'i> From<xwfd::EventKind<'i>> for EventKind {
    fn from(ev_kind: xwfd::EventKind<'i>) -> Self {
        match ev_kind {
            xwfd::EventKind::Call { args_set } => EventKind::Call {
                args_set: args_set.to_vec(),
            },
            // EventKind::Read => {}
            // EventKind::Write { .. } => {}
            // EventKind::OpenStreams => {}
            // EventKind::CloseStreams => {}
            // EventKind::Subscribe { .. } => {}
            // EventKind::Unsubscribe => {}
            // EventKind::Borrow => {}
            // EventKind::Release => {}
            // EventKind::Introspect => {}
            xwfd::EventKind::CallResults(results) => EventKind::CallResults(
                results
                    .iter()
                    .map(|r| r.map(|slice| slice.to_owned()))
                    .collect(),
            ),
            // EventKind::ReadResults(_) => {}
            // EventKind::WriteResults(_) => {}
            // EventKind::OpenStreamsResults(_) => {}
            // EventKind::CloseStreamsResults(_) => {}
            // EventKind::SubscribeResults(_) => {}
            // EventKind::RateChangeResults(_) => {}
            // EventKind::UnsubscribeResults(_) => {}
            // EventKind::BorrowResults(_) => {}
            // EventKind::ReleaseResults(_) => {}
            // EventKind::IntrospectResults(_) => {}
            // EventKind::StreamUpdates(_) => {}
            // EventKind::DiscoverNodes => {}
            // EventKind::NodeInfo(_) => {}
            // EventKind::Heartbeat(_) => {}
            // EventKind::Forward => {}
            _ => unimplemented!(),
        }
    }
}

impl Display for EventKind {
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
            EventKind::CallResults(results) => write!(f, "CallResults[{:?}]", results),
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
