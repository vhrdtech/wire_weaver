/// Requests are sent to the Link by the initiator of an exchange, which can be any node on the Link.
/// One or several Responses are sent back for each kind of request.
///
/// In case of subscribing to property updates or streams, responses will continue to arrive
/// until unsubscribed, stream exhausted or closed or one of the nodes rebooting.
///
/// After subscribers node reboot, one or more responses may arrive, until publishing nodes notices
/// subscribers reboot, unless subscribed again.
///
/// This is a generic type, see actual implementations:
/// * [vlu4, borrowed, no_std, zero copy](crate::serdes::xwfd::request::XpiRequest)
/// * [vlu4, owned, std]()
///
/// Replies are sent to the Link in response to requests.
/// For subscriptions and streams many replies will be sent asynchronously.
///
/// Each request will result in one or more replies (or zero if loss occurred). This is due to:
/// buffer space available, sync vs async resources, priorities and other factors.
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum XpiGenericEventKind<
    // SL,  // must be a slice, e.g. &'req [u8] or Vec<u8>
    VSL,  // must be an array of slices, e.g. Vlu4Vec<'req, &'req [u8]> or Vec<Vec<u8>>
    VR,   // must be an array of rates, e.g. Vlu4Vec<'req, Rate> or Vec<Rate>
    VRSL, // must be an array of Result<slice>, e.g. Vlu4Vec<'rep, Result<&'rep [u8], FailReason>>
    VRU,  // must be an array of Result<()>, e.g. Vlu4Vec<'rep, Result<(), FailReason>>
    VRI,  // must be an array of Result<ResourceInfo>
    N,    // Node info
    H,    // Heartbeat info
> {
    /// Request binary descriptor block from a node.
    /// Descriptor block is a compiled binary version of a vhL source.
    /// It carries all the important information that is needed to interact with the node.
    /// Including:
    /// * All the data types, also those coming from dependencies
    /// * Unique IDs of all the dependencies and of itself (everything must be published to the
    ///     repository before binary block can be compiled or dirty flag can be set for dev)
    /// * All the xPI blocks with strings (names, descriptions), examples and valid values.
    ///
    /// [Format description (notion)](https://www.notion.so/vhrdtech/Descriptor-block-d0fb717035574255a9baebdb18b8a4f2)
    //GetDescriptorBlock, -> move to a stream_out<chunk> resource, can also add separate const property with a link to the vhL source

    /// Call one or more methods.
    /// Results in [XpiReply::FnCallFailed] or [XpiReply::FnReturn] for each method.
    Call {
        /// Arguments must be serialized with the chosen [Wire Format](https://github.com/vhrdtech/vhl/blob/master/book/src/wire_formats/wire_formats.md)
        /// Need to get buffer for serializing from user code, which decides how to handle memory
        args_set: VSL,
    },

    // /// Perform f(g(h(... (args) ...))) call on the destination node, saving
    // /// round trip request and replies.
    // /// Arguments must be compatible across all the members of a chain.
    // /// One response is sent back for the outer most function.
    // /// May not be supported by all nodes.
    // /// Do not cover all the weird use cases, so maybe better be replaced with full-blown expression
    // /// executor only were applicable and really needed?
    // ChainCall { args: SL },
    /// Read one or more resources.
    /// Reading several resources at once is more efficient as only one req-rep is needed in best case.
    /// Resources that support reads are: const, ro, ro + stream, rw, rw + stream
    Read,

    /// Write one or more resources.
    /// Resources that support writes are: wo, wo + stream, rw, rw + stream, stream_in<T> when open only.
    Write {
        /// Values to be written, must be the same length as a resource set in order
        values: VSL,
    },

    /// Open one or more streams for read, writes, publishing or subscribing.
    /// stream_in<T> can be written into or published to.
    /// It is only a hint to codegen to create more useful abstractions, there is no functional
    /// difference between publishing or writing.
    ///
    /// stream_out<T> can be read or subscribed to.
    /// In contrast with writing vs publishing, reading is different from subscribing, as only
    /// one result is returned on read, but one or many after subscribing.
    ///
    /// Only opened streams can be written into, read from or subscribed to.
    /// Stream thus have a start and an end in contrast to properties with a +observe modifier.
    /// Stream are also inherently Borrowable (so writing stream_in<T> is equivalent to Cell<stream_in<T>>).
    /// When opening a closed stream, it is automatically borrowed. Opening an open stream returns an error.
    OpenStreams,

    /// Closes one or more streams.
    /// Can be used as an end mark for writing a file for example.
    CloseStreams,

    /// Subscribe to property changes or streams.
    /// Resources must be be rw + stream, ro + stream or stream_out<T>.
    ///
    /// To change rates, subscribe again to the same or different set of resources.
    ///
    /// Publishers must avoid emitting changes with higher than requested rates.
    Subscribe {
        /// For each uri there must be a specified [Rate] provided.
        rates: VR,
    },

    // /// Request a change in properties observing or stream publishing rates.
    // ChangeRates {
    //     /// For each uri there must be a specified [Rate] provided.
    //     rates: &'req [Rate],
    // },
    /// Unsubscribe from one or many resources, unsubscribing from a stream do not close it,
    /// but releases a borrow, so that someone else can subscribe and continue receiving data.
    Unsubscribe,

    /// Borrow one or many resources for exclusive use. Only work ons streams and Cell<T> resources.
    /// Other nodes will receive an error if they attempt to access borrowed resources.
    ///
    /// Nodes may implement more logic to allow or block borrowing of a resource.
    /// For example expecting a correct configuration or a key first.
    /// /main {
    ///     /key<wo String> {}
    ///     /dangerous_things<Cell<_>> {
    ///         /wipe_data<fn()> {}
    ///     }
    /// }
    /// In this example one would first have to write a correct key and then try to borrow
    /// /dangerous_things. If the key is incorrect, borrow can be rejected. Stronger security
    /// algorithms can probably be also implemented to granularly restrict access.
    /// Link between the nodes can also be encrypted, with a common key or a set of keys between all nodes.
    /// Encryption is out of scope of this document though.
    ///
    /// Might be a good idea to introduce some limitation on how many borrows can be made from one node.
    /// Depends on the kind of resource. Do not protect against malicious attempts, as node ids can be
    /// faked, but can prevent bugs.
    Borrow,

    /// Release resources for others to use.
    Release,

    /// Get information about resources.
    /// Type information for all resources.
    /// In addition:
    /// * Cell<T>: whether resource is borrowed or not.
    /// * stream_in<T> or stream_out<T>: whether stream is opened or
    /// not (when implicit Cell is already borrowed) + subscribers info + rates.
    /// * +stream: subscribers info + rates
    /// * fn: nothing at the moment
    /// * const: nothing at the moment
    /// * array of resources: size of the array
    Introspect,

    /// Result of an each call
    CallResults(VRSL),

    /// Result of an each read.
    ReadResults(VRSL),

    /// Result of an each write (only lossless?)
    WriteResults(VRU),

    /// Result of an attempt to open a stream.
    /// If stream was closed before (and inherently not borrowed), Borrow(Ok(())) is received,
    /// followed by OpenStream(Ok(()))
    OpenStreamsResults(VRU),

    /// Result of an attempt to close a stream or unrecoverable loss in lossless mode (priority > 0).
    /// If stream was open before (and inherently borrowed by self node), Close(Ok(())) is received,
    /// followed by Release(Ok(())).
    CloseStreamsResults(VRU),

    /// Result of an attempt to subscribe to a stream or observable property
    /// On success Some(current value) is returned for a property, first available item is returned
    /// for streams, if available during subscription time.
    /// array of results with 0 len as an option
    SubscribeResults(VRSL),

    /// Result of a request to change observing / publishing rate.
    RateChangeResults(VRU),

    /// Result of an attempt to unsubscribe from a stream or from an observable property.
    /// Unsubscribing twice will result in an error.
    UnsubscribeResults(VRU),

    /// Result of a resource borrow
    BorrowResults(VRU),
    /// Result of a resource release
    ReleaseResults(VRU),

    /// Result of an Introspect request
    IntrospectResults(VRI),

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
    StreamUpdates(VSL),

    /// Broadcast request to all the nodes to announce themselves.
    /// Up to the user how to actually implement this (for example zeroconf or randomly
    /// delayed transmissions on CAN Bus if unique IDs wasn't assigned yet).
    DiscoverNodes,
    /// Sent by nodes in response to [XpiRequest::DiscoverNodes]. Received by everyone else.
    NodeInfo(N),
    /// Sent by all nodes periodically, received by all nodes.
    /// Must be sent with maximum lossy priority.
    /// If emergency stop messages exist in a system, heartbeats should be sent with the next lower priority.
    Heartbeat(H),

    /// Forward event to another node, allowing node to node routing with manual path selection.
    Forward,
}

impl<VSL, VR, VRSL, VRU, VRI, N, H> XpiGenericEventKind<VSL, VR, VRSL, VRU, VRI, N, H> {
    pub fn discriminant(&self) -> XpiEventDiscriminant {
        use XpiEventDiscriminant::*;
        match self {
            // Request like event
            XpiGenericEventKind::Call { .. } => Call,
            // XpiGenericEventKind::ChainCall { .. } => ChainCall,
            XpiGenericEventKind::Read => Read,
            XpiGenericEventKind::Write { .. } => Write,
            XpiGenericEventKind::OpenStreams => OpenStreams,
            XpiGenericEventKind::CloseStreams => CloseStreams,
            XpiGenericEventKind::Subscribe { .. } => Subscribe,
            XpiGenericEventKind::Unsubscribe => Unsubscribe,
            XpiGenericEventKind::Borrow => Borrow,
            XpiGenericEventKind::Release => Release,
            XpiGenericEventKind::Introspect => Introspect,

            // Reply like events
            XpiGenericEventKind::CallResults(_) => CallResults,
            XpiGenericEventKind::ReadResults(_) => ReadResults,
            XpiGenericEventKind::WriteResults(_) => WriteResults,
            XpiGenericEventKind::OpenStreamsResults(_) => OpenStreamsResults,
            XpiGenericEventKind::CloseStreamsResults(_) => CloseStreamsResults,
            XpiGenericEventKind::SubscribeResults(_) => SubscribeResults,
            XpiGenericEventKind::RateChangeResults(_) => RateChangeResults,
            XpiGenericEventKind::UnsubscribeResults(_) => UnsubscribeResults,
            XpiGenericEventKind::BorrowResults(_) => BorrowResults,
            XpiGenericEventKind::ReleaseResults(_) => ReleaseResults,
            XpiGenericEventKind::IntrospectResults(_) => IntrospectResults,

            // Multicast / Broadcast like events
            XpiGenericEventKind::StreamUpdates(_) => StreamUpdates,
            XpiGenericEventKind::DiscoverNodes => DiscoverNodes,
            XpiGenericEventKind::NodeInfo(_) => NodeInfo,
            XpiGenericEventKind::Heartbeat(_) => Heartbeat,

            // Special events
            XpiGenericEventKind::Forward => Forward,
        }
    }
}

/// Same as XpiGenericRequestKind but without data.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum XpiEventDiscriminant {
    Call = 0,
    Read = 1,
    Write = 2,
    OpenStreams = 3,
    CloseStreams = 4,
    Subscribe = 5,
    Unsubscribe = 6,
    Borrow = 7,
    Release = 8,
    Introspect = 9,
    //ChainCall = 10,
    CallResults = 16,
    ReadResults = 17,
    WriteResults = 18,
    OpenStreamsResults = 19,
    CloseStreamsResults = 20,
    SubscribeResults = 21,
    UnsubscribeResults = 22,
    BorrowResults = 23,
    ReleaseResults = 24,
    IntrospectResults = 25,
    RateChangeResults = 31,

    StreamUpdates = 32,
    DiscoverNodes = 33,
    NodeInfo = 34,
    Heartbeat = 35,

    Forward = 48,
}
