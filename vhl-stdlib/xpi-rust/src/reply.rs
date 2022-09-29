use crate::resource_set::XpiGenericResourceSet;

/// Replies are sent to the Link in response to requests.
/// One request can result in one or more replies.
/// For subscriptions and streams many replies will be sent asynchronously.
#[derive(Clone, Debug)]
pub struct XpiGenericReply<U, MU, VSL, VRSL, VRU, VRI, ID> {
    /// Set of resources that are considered in this reply
    pub resource_set: XpiGenericResourceSet<U, MU>,
    /// Kind of reply
    pub kind: XpiGenericReplyKind<VSL, VRSL, VRU, VRI>,
    /// Original request id used to map responses to requests.
    /// For StreamsUpdates use previous id + 1 and do not map to requests.
    pub request_id: ID,
}

/// Reply to a previously made request.
///
/// Each request will result in one or more replies (or zero if loss occurred). This is due to:
/// buffer space available, sync vs async resources, priorities and other factors.
#[derive(Copy, Clone, Debug)]
// #[enum_kind(XpiReplyKindKind)] simple enough to do by hand and helps with code completion
pub enum XpiGenericReplyKind<
    VSL,  // must be an array of slices, e.g. Vlu4Vec<'rep, &'rep [u8]> or Vec<Vec<u8>>
    VRSL, // must be an array of Result<slice>, e.g. Vlu4Vec<'rep, Result<&'rep [u8], FailReason>>
    VRU,  // must be an array of Result<()>, e.g. Vlu4Vec<'rep, Result<(), FailReason>>
    VRI,  // must be an array of Result<ResourceInfo>
> {
    /// Result of an each call
    CallComplete(VRSL),

    /// Result of an each read.
    ReadComplete(VRSL),

    /// Result of an each write (only lossless?)
    WriteComplete(VRU),

    /// Result of an attempt to open a stream.
    /// If stream was closed before (and inherently not borrowed), Borrow(Ok(())) is received,
    /// followed by OpenStream(Ok(()))
    OpenStream(VRU),

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
    StreamUpdate(VSL),

    /// Result of an attempt to close a stream or unrecoverable loss in lossless mode (priority > 0).
    /// If stream was open before (and inherently borrowed by self node), Close(Ok(())) is received,
    /// followed by Release(Ok(())).
    CloseStream(VRU),

    /// Result of an attempt to subscribe to a stream or observable property
    /// On success Some(current value) is returned for a property, first available item is returned
    /// for streams, if available during subscription time.
    /// array of results with 0 len as an option
    Subscribe(VRSL),

    /// Result of a request to change observing / publishing rate.
    RateChange(VRU),

    /// Result of an attempt to unsubscribe from a stream of from an observable property.
    /// Unsubscribing twice will result in an error.
    Unsubscribe(VRU),

    /// Result of a resource borrow
    Borrow(VRU),
    /// Result of a resource release
    Release(VRU),

    /// Result of an Introspect request
    Introspect(VRI),
}

impl<VSL, VRSL, VRU, VRI> XpiGenericReplyKind<VSL, VRSL, VRU, VRI> {
    pub fn discriminant(&self) -> XpiReplyDiscriminant {
        use XpiReplyDiscriminant::*;
        match self {
            XpiGenericReplyKind::CallComplete(_) => CallComplete,
            XpiGenericReplyKind::ReadComplete(_) => ReadComplete,
            XpiGenericReplyKind::WriteComplete(_) => WriteComplete,
            XpiGenericReplyKind::OpenStream(_) => OpenStream,
            XpiGenericReplyKind::StreamUpdate(_) => StreamUpdate,
            XpiGenericReplyKind::CloseStream(_) => CloseStream,
            XpiGenericReplyKind::Subscribe(_) => Subscribe,
            XpiGenericReplyKind::RateChange(_) => RateChange,
            XpiGenericReplyKind::Unsubscribe(_) => Unsubscribe,
            XpiGenericReplyKind::Borrow(_) => Borrow,
            XpiGenericReplyKind::Release(_) => Release,
            XpiGenericReplyKind::Introspect(_) => Introspect
        }
    }
}

/// Same as XpiGenericReplyKind but without data.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum XpiReplyDiscriminant {
    CallComplete = 0,
    ReadComplete = 1,
    WriteComplete = 2,
    OpenStream = 3,
    StreamUpdate = 4,
    CloseStream = 5,
    Subscribe = 6,
    RateChange = 7,
    Unsubscribe = 8,
    Borrow = 9,
    Release = 10,
    Introspect = 11,
}
