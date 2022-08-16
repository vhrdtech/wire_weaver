use crate::serdes::vlu4::Vlu4SliceArray;
use crate::serdes::xpi_vlu4::addressing::{NodeSet, RequestId, XpiResourceSet};
use crate::serdes::xpi_vlu4::error::FailReason;
use crate::serdes::xpi_vlu4::NodeId;
use crate::serdes::xpi_vlu4::request::XpiRequestKind;
use crate::serdes::xpi_vlu4::resource_info::ResourceInfo;

/// Replies are sent to the Link in response to requests.
/// One request can result in one or more replies.
/// For subscriptions and streams many replies will be sent asynchronously.
#[derive(Copy, Clone, Debug)]
pub struct XpiReply<'rep> {
    /// Source node id that yielded reply
    pub source: NodeId,
    /// Destination node or nodes
    pub destination: NodeSet<'rep>,
    /// Kind of reply
    pub kind: XpiRequestKind<'rep>,
    /// Set of resources that are considered in this reply
    pub resource_set: XpiResourceSet<'rep>,
    /// Original request id used to map responses to requests.
    /// None for StreamsUpdates kind.
    pub request_id: Option<RequestId>,
}

/// Reply to a previously made request
/// Each reply must also be linked with:
/// request id that was sent initially
/// Source node id
#[derive(Copy, Clone, Debug)]
pub enum XpiReplyKind<'rep> {
    /// Result of an each call
    CallComplete(Result<&'rep [u8], FailReason>),

    /// Result of an each read.
    ReadComplete(Result<Vlu4SliceArray<'rep>, FailReason>),

    /// Result of an each read
    WriteComplete(Result<(), FailReason>),

    /// Result of an attempt to open a stream.
    /// If stream was closed before (and inherently not borrowed), Borrow(Ok(())) is received,
    /// followed by OpenStream(Ok(()))
    OpenStream(Result<(), FailReason>),

    /// Changed property or new element of a stream.
    /// request_id for this case is None, as counter may wrap many times while subscriptions are active.
    /// Mapping is straight forward without a request_id, since uri for each resource is known.
    /// Distinguishing between different updates is not needed as in case of 2 function calls vs 1 for example.
    ///
    /// Updates may be silently lost if lossy mode is selected, more likely so with lower priority.
    ///
    /// Updates are very unlikely to be lost in lossless mode, unless underlying channel is destroyed
    /// or memory is exceeded, in which case only an error can be reported to flag the issue.
    /// If lossless channel is affected, CloseStream is yielded with a failure reason indicated in it.
    StreamUpdate(&'rep [u8]),

    /// Result of an attempt to close a stream or unrecoverable loss in lossless mode (priority > 0).
    /// If stream was open before (and inherently borrowed by self node), Close(Ok(())) is received,
    /// followed by Release(Ok(())).
    CloseStream(Result<(), FailReason>),

    /// Result of an attempt to subscribe to a stream or observable property
    /// On success Some(current value) is returned for a property, first available item is returned
    /// for streams, if available during subscription time.
    Subscribe(Result<Option<&'rep [u8]>, FailReason>),

    /// Result of a request to change observing / publishing rate.
    RateChange(Result<(), FailReason>),

    /// Result of an attempt to unsubscribe from a stream of from an observable property.
    /// Unsubscribing twice will result in an error.
    Unsubscribe(Result<(), FailReason>),

    /// Result of a resource borrow
    Borrow(Result<(), FailReason>),
    /// Result of a resource release
    Release(Result<(), FailReason>),

    /// Result of an Introspect request
    Introspect(Result<ResourceInfo<'rep>, FailReason>),
}