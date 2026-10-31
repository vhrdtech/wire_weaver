//! Types of the local trait an API gets with `ww_codegen!(crate :: Api, local_trait = "Name")` (CLT-57): the
//! trait is implemented by the generated client and by local types, so a call site generic over it works with both.
//! The types live in [wire_weaver::call] (SRV-48), so an API crate names them without the client; they are
//! re-exported here.

use crate::{Priority, RawStreamEvent, Stream, StreamError};
use std::time::Duration;
use wire_weaver::CallContext;
use ww_client_server::StreamSideband;

pub use wire_weaver::call::{BoxFuture, ItemStream, item_stream};
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;

impl<T: DeserializeShrinkWrapOwned + Send + 'static> Stream<T> {
    /// This stream as the [ItemStream] of a local trait: each [Stream::recv], ending when the device closes the
    /// stream or the handle's channel is gone ([StreamError::Closed]); any other error is the last item, as a
    /// [wire_weaver::call::Error] with the [StreamError] as its cause.
    pub fn into_item_stream(self) -> ItemStream<T> {
        Box::pin(futures_lite::stream::unfold(
            Some(self),
            |stream| async move {
                let mut stream = stream?;
                match stream.recv().await {
                    Ok(item) => Some((Ok(item), Some(stream))),
                    Err(StreamError::Closed)
                    | Err(StreamError::UnexpectedEvent(RawStreamEvent::Sideband(
                        StreamSideband::Close,
                    ))) => None,
                    Err(e) => Some((Err(e.into()), None)),
                }
            },
        ))
    }
}

/// A stream's error as the item error of an [ItemStream]: [Internal](wire_weaver::call::ErrorKind::Internal), the
/// [StreamError] kept as the cause.
impl From<StreamError> for wire_weaver::call::Error {
    fn from(e: StreamError) -> Self {
        wire_weaver::call::Error::internal(e)
    }
}

/// `with_call_context` of the prepared calls: `cx`'s priority unless it is the default, its deadline as the timeout.
pub(crate) fn apply_call_context<P>(
    prepared: P,
    cx: &CallContext,
    priority: impl FnOnce(P, Priority) -> P,
    timeout: impl FnOnce(P, Duration) -> P,
) -> P {
    let prepared = if cx.priority != Priority::NORMAL {
        priority(prepared, cx.priority)
    } else {
        prepared
    };
    match cx.deadline {
        Some(deadline) => timeout(prepared, Duration::from_millis(deadline.get().into())),
        None => prepared,
    }
}
