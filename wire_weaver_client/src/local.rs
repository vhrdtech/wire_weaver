//! Types of the local trait an API gets with `ww_codegen!(crate :: Api, local_trait = "Name")` (CLT-57): the
//! trait is implemented by the generated client and by local types, so a call site generic over it works with both.

use crate::{Stream, StreamError};
use std::future::Future;
use std::pin::Pin;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;

/// Future of a function of a local trait's `Dyn` companion.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Items of a stream opened through a local trait: from a device ([Stream::into_item_stream]) or from a local
/// implementation ([item_stream]). Ends when the stream is closed.
pub type ItemStream<T> = Pin<Box<dyn futures_lite::Stream<Item = Result<T, StreamError>> + Send>>;

/// An [ItemStream] of a local implementation, e.g. from `futures_lite::stream::iter` or a channel receiver.
pub fn item_stream<T>(
    stream: impl futures_lite::Stream<Item = Result<T, StreamError>> + Send + 'static,
) -> ItemStream<T> {
    Box::pin(stream)
}

impl<T: DeserializeShrinkWrapOwned + Send + 'static> Stream<T> {
    /// This stream as the [ItemStream] of a local trait: each [Stream::recv], ending at [StreamError::Closed]; any
    /// other error is the last item.
    pub fn into_item_stream(self) -> ItemStream<T> {
        Box::pin(futures_lite::stream::unfold(
            Some(self),
            |stream| async move {
                let mut stream = stream?;
                match stream.recv().await {
                    Ok(item) => Some((Ok(item), Some(stream))),
                    Err(StreamError::Closed) => None,
                    Err(e) => Some((Err(e), None)),
                }
            },
        ))
    }
}
