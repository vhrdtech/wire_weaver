use std::net::IpAddr;

use smallvec::SmallVec;

pub struct Event {
    pub source: Address,
    pub destination: Address,
    pub base_nrl: Option<Nrl>,
    pub kind: EventKind,
    pub seq: Option<u32>,
}

pub enum EventKind {
    // Single(Action),
    Requests {
        /// If multiple actions are targeted at one sub level
        // common_nrl: Option<Nrl>,
        actions: SmallVec<[Request; 1]>,
        bail_on_error: bool,
    },
    // Trait {
    //     descriptor: TraitDescriptor,
    //     /// If trait is implemented several times, select which one to use
    //     selector: Option<Nrl>,
    //     action: Action,
    // },
    // TraitMany {
    //     descriptor: TraitDescriptor,
    //     /// If trait is implemented several times, select which one to use
    //     selector: Option<Nrl>,
    //     actions: Vec<Action>,
    // },
    Replies {
        results: SmallVec<[Reply; 1]>,
    },
}

pub struct Address {
    pub ip_addr: IpAddr,
    pub ip_port: u16,
    // similar to node_id, to reuse the same connection multiple times
    pub virtual_port: u16,
    pub wire_format: WireFormat,
}

pub enum WireFormat {
    MessagePack,
    Wfs,
    Wfd,
}

pub struct TraitDescriptor {
    pub trait_id: u64,
}

pub type Nrl = SmallVec<[u32; 3]>;

pub struct Request {
    pub tr: Option<TraitDescriptor>,
    pub nrl: Nrl,
    pub reply_ack: ReplyAck,
    pub kind: RequestKind,
}

pub enum RequestKind {
    Call { args: Vec<u8> },
    Read,
    Write { value: Vec<u8> },
    OpenStream,
    CloseStream,
    Subscribe,
    Unsubscribe,
    Borrow,
    Release,
    Introspect,
    Heartbeat,
}

pub struct Reply {
    pub nrl: Nrl,
    pub kind: ReplyKind,
}

pub enum ReplyKind {
    CallResult { ret_value: Result<Vec<u8>, Error> },
    ReadResult { value: Result<Vec<u8>, Error> },
    WriteResult { status: Result<(), Error> },
    OpenStreamResult { status: Result<(), Error> },
    StreamUpdate { data: Result<Vec<u8>, Error> },
    CloseStreamResult { status: Result<(), Error> },
    SubscribeResult { status: Result<(), Error> },
    RateChangeResult { status: Result<(), Error> },
    UnsubscribeResult { status: Result<(), Error> },
    BorrowResult { status: Result<(), Error> },
    ReleaseResult { status: Result<(), Error> },
    IntrospectResult { vhl: Result<Vec<u8>, Error> },
    Heartbeat { payload: () },
}

pub enum Error {
    XpiError(()),
    IoError,
}

pub enum ReplyAck {
    /// Always reply to requests
    Ack,
    /// Only reply if request resulted in an error
    Nack,
    /// Do not reply regardless of a result
    Ignore,
}
