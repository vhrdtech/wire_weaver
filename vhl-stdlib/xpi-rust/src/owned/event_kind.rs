use crate::error::XpiError;
use crate::event_kind::XpiGenericEventKind;
use crate::owned::convert_error::ConvertError;
use crate::owned::{Rate, ResourceInfo};
use crate::xwfd;
use std::fmt::{Display, Formatter};
use vhl_stdlib::serdes::nibble_buf::NibbleBufOwned;
use vhl_stdlib::serdes::NibbleBufMut;

pub type EventKind = XpiGenericEventKind<
    Vec<NibbleBufOwned>,
    Vec<Rate>,
    Vec<Result<NibbleBufOwned, XpiError>>,
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
                nwr.put_vec_with(|vb| args_set.iter().try_for_each(|args| vb.put(args)))?;
            }
            EventKind::Read => {}
            EventKind::Write { values } => {
                nwr.put_vec_with(|vb| values.iter().try_for_each(|value| vb.put(value)))?;
            }
            EventKind::OpenStreams => {}
            EventKind::CloseStreams => {}
            EventKind::Subscribe { rates } => {}
            EventKind::Unsubscribe => {}
            EventKind::Borrow => {}
            EventKind::Release => {}
            EventKind::Introspect => {}
            EventKind::CallResults(results) => {
                nwr.put_vec_with(|vb| results.iter().try_for_each(|result| vb.put(result)))?;
            }
            EventKind::ReadResults(results) => {
                nwr.put_vec_with(|vb| results.iter().try_for_each(|result| vb.put(result)))?;
            }
            // EventKind::WriteResults(_) => {}
            // EventKind::OpenStreamsResults(_) => {}
            // EventKind::CloseStreamsResults(_) => {}
            EventKind::SubscribeResults(immediate_updates) => {
                nwr.put_vec_with(|vb| {
                    immediate_updates.iter().try_for_each(|value| vb.put(value))
                })?;
            }
            // EventKind::RateChangeResults(_) => {}
            // EventKind::UnsubscribeResults(_) => {}
            // EventKind::BorrowResults(_) => {}
            // EventKind::ReleaseResults(_) => {}
            // EventKind::IntrospectResults(_) => {}
            // EventKind::StreamUpdates(_) => {}
            // EventKind::DiscoverNodes => {}
            // EventKind::NodeInfo(_) => {}
            EventKind::Heartbeat(_) => {}
            // EventKind::Forward => {}
            u => unimplemented!("{u:?}"),
        }
        Ok(())
    }
}

impl<'i> From<xwfd::EventKind<'i>> for EventKind {
    fn from(ev_kind: xwfd::EventKind<'i>) -> Self {
        match ev_kind {
            xwfd::EventKind::Call { args_set } => EventKind::Call {
                args_set: args_set.iter().map(|nb| nb.to_nibble_buf_owned()).collect(),
            },
            xwfd::EventKind::Read => EventKind::Read,
            xwfd::EventKind::Write { values } => EventKind::Write {
                values: values.iter().map(|nb| nb.to_nibble_buf_owned()).collect(),
            },
            xwfd::EventKind::OpenStreams => EventKind::OpenStreams,
            xwfd::EventKind::CloseStreams => EventKind::CloseStreams,
            xwfd::EventKind::Subscribe { .. } => EventKind::Subscribe { rates: vec![] },
            xwfd::EventKind::Unsubscribe => EventKind::Unsubscribe,
            xwfd::EventKind::Borrow => EventKind::Borrow,
            xwfd::EventKind::Release => EventKind::Release,
            xwfd::EventKind::Introspect => EventKind::Introspect,
            xwfd::EventKind::CallResults(results) => EventKind::CallResults(
                results
                    .iter()
                    .map(|r| r.map(|nb| nb.to_nibble_buf_owned()))
                    .collect(),
            ),
            xwfd::EventKind::ReadResults(results) => EventKind::ReadResults(
                results
                    .iter()
                    .map(|r| r.map(|nb| nb.to_nibble_buf_owned()))
                    .collect(),
            ),
            xwfd::EventKind::WriteResults(results) => {
                EventKind::WriteResults(results.iter().collect())
            }
            xwfd::EventKind::OpenStreamsResults(results) => {
                EventKind::OpenStreamsResults(results.iter().collect())
            }
            xwfd::EventKind::CloseStreamsResults(results) => {
                EventKind::CloseStreamsResults(results.iter().collect())
            }
            xwfd::EventKind::RateChangeResults(results) => {
                EventKind::RateChangeResults(results.iter().collect())
            }
            xwfd::EventKind::UnsubscribeResults(results) => {
                EventKind::UnsubscribeResults(results.iter().collect())
            }
            xwfd::EventKind::BorrowResults(results) => {
                EventKind::BorrowResults(results.iter().collect())
            }
            xwfd::EventKind::ReleaseResults(results) => {
                EventKind::ReleaseResults(results.iter().collect())
            }
            xwfd::EventKind::SubscribeResults(immediate_values) => EventKind::SubscribeResults(
                immediate_values
                    .iter()
                    .map(|r| r.map(|nb| nb.to_nibble_buf_owned()))
                    .collect(),
            ),
            xwfd::EventKind::IntrospectResults(_values) => EventKind::IntrospectResults(vec![]),
            xwfd::EventKind::StreamUpdates(slices) => {
                EventKind::StreamUpdates(slices.iter().map(|nb| nb.to_nibble_buf_owned()).collect())
            }
            xwfd::EventKind::DiscoverNodes => EventKind::DiscoverNodes,
            xwfd::EventKind::NodeInfo(_) => EventKind::NodeInfo(()),
            xwfd::EventKind::Heartbeat(_) => EventKind::Heartbeat(123),
            xwfd::EventKind::Forward => EventKind::Forward,
        }
    }
}

impl Display for EventKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            EventKind::Call { args_set } => write!(f, "Call({:?})", args_set),
            EventKind::Read => write!(f, "Read"),
            EventKind::Write { values } => write!(f, "Write({:?}", values),
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
