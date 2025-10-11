#![cfg_attr(not(feature = "std"), no_std)]

pub mod util;

use wire_weaver::prelude::*;
use ww_version::{CompactVersion, FullVersion};

#[cfg(feature = "std")]
use ww_version::FullVersionOwned;

pub const FULL_VERSION: FullVersion = full_version!();

/// Represents one operation (call, read, write, etc.) to be performed on a resource.
/// 3 modes of addressing is supported:
/// * Explicit - only the number path to a resource is used, smallest size
/// * CompactVersion - request to a trait resource, for commonly used traits that are used often and have an ID assigned TODO: add git link to global trait list
/// * FullVersion - request to a trait resource defined in an arbitrary Rust crate, full crate name, and it's version is used as an ID
#[derive_shrink_wrap]
#[owned = "std"]
#[derive(Debug)]
pub struct Request<'i> {
    /// Request ID, starting from 1 and wrapping back to 1 that allows to map responses to requests.
    /// 0 means no answer is expected.
    pub seq: u16,

    /// Specifies whether resource is addressed explicitly, using full path to it or through global trait ID.
    pub path_kind: PathKind<'i>,

    /// Action being requested
    pub kind: RequestKind<'i>,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
pub enum PathKind<'i> {
    /// Full path to a resource, regardless whether it is in a trait or not.
    Absolute { path: RefVec<'i, UNib32> },

    /// Request is for a trait implemented at root level, path_from_trait is used to identify a resource inside the trait.
    /// CompactVersion consists of 3 UNib32's, so the smallest additional size of such call if all numbers are <= 7 is 2 bytes.
    GlobalCompact {
        gid: CompactVersion,
        path_from_trait: RefVec<'i, UNib32>,
    },

    /// Request is for a trait implemented at root level, path_from_trait is used to identify a resource inside the trait.
    /// This kind of request is the biggest, because full crate name is used.
    GlobalFull {
        gid: FullVersion<'i>,
        path_from_trait: RefVec<'i, UNib32>,
    },
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
pub enum RequestKind<'i> {
    /// Call a method using provided arguments. Arguments are put into a struct and serialized using shrink_wrap to obtain this byte array.
    Call {
        args: RefVec<'i, u8>,
    },

    /// Read a property.
    Read,
    // ReadDefault,
    // ReadMany,
    /// Write property or stream down. Property value is serialized fully into a byte array using shrink_wrap.
    /// Objects of a stream are also serialized in full and sent as one unit.
    Write {
        data: RefVec<'i, u8>,
    },
    // WriteDefault,
    // WriteMany,
    OpenStream,
    CloseStream,

    /// Subscribe to property changes
    Subscribe,
    /// Unsubscribe from property changes
    Unsubscribe,

    ChangeRate {
        shaper_config: ShaperConfig,
    },

    Introspect,
    // Version,
    // Borrow,
    // Release,
    // Heartbeat,
}

#[derive_shrink_wrap]
#[owned = "std"]
#[derive(Debug)]
pub struct Event<'i> {
    pub seq: u16,
    // path
    pub result: Result<EventKind<'i>, Error>,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
pub enum EventKind<'i> {
    ReturnValue {
        data: RefVec<'i, u8>,
    },

    ReadValue {
        data: RefVec<'i, u8>,
    },
    Written,

    StreamOpened {
        // When subscribing through trait interface, this path is used later to match stream updates to an original request
        path: Vec<UNib32>,
    },
    // If stream is a sequence of bytes, can be used to delimit frames or send other data out of band
    // StreamDelimiter { path: Vec<nib16>, user_data: u8 },
    // TODO: Add Option<SizeHint>
    StreamUpdate {
        path: Vec<UNib32>,
        data: RefVec<'i, u8>,
    },
    StreamClosed {
        path: Vec<UNib32>,
    },

    Subscribed {
        // When subscribing through trait interface, this path is used later to match stream updates to an original request
        path: Vec<UNib32>,
    },
    Unsubscribed {
        path: Vec<UNib32>,
    },

    RateChanged,

    Introspect {
        // ww_self version?
        ww_self_bytes: RefVec<'i, u8>,
    },
    // Version {
    //     protocol_id: u32,
    //     version: Version<'i>,
    // },
    // Heartbeat { data: Vec<u8> },
    // Borrowed,
    // Released,
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[self_describing]
#[derive(Debug)]
pub enum Error {
    // Tried to unsubscribe twice from a resource
    // AlreadyUnsubscribed,
    // Tried to open a stream twice
    // StreamIsAlreadyOpen,
    // Tried to close a stream twice
    // StreamIsAlreadyClosed,
    /// Sent a RequestKind that doesn't make sense for a particular resource
    OperationNotSupported,
    /// Tried to access a path that doesn't exist
    BadPath,
    /// Tried to access a resource array using out of bounds index
    BadIndex,
    /// Expected an array index in the resource path, but got None instead
    ExpectedArrayIndexGotNone,
    /// Tried to deserialize UNib32 from the resource path, but got an error
    ArrayIndexDesFailed,

    // Tried to get a byte slice out of Call, Write args, but shrink wrap returned an error, most likely malformed request.
    // SliceGetFailed,
    ArgsDesFailed,
    PathDesFailed,
    PropertyDesFailed,
    ResponseSerFailed,
    /// Request is good, but requested operation is not yet implemented
    OperationNotImplemented,
    /// Tried to read a property with request seq number set to 0, meaning no response is expected
    ReadPropertyWithSeqZero,
    /// Returned if only absolute paths are handled (on very resource constrained nodes)
    PathKindNotSupported,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Debug, Copy, Clone)]
pub enum ShaperConfig {
    NoLimit,
    MaxBitrate { bytes_per_s: u32 },
    MaxRate { events_per_s: u32 },
}

#[cfg(feature = "std")]
impl PathKind<'_> {
    pub fn make_owned(&self) -> Result<PathKindOwned, wire_weaver::shrink_wrap::Error> {
        let path = match self {
            PathKind::Absolute { path } => PathKindOwned::Absolute {
                path: path.iter().collect::<Result<Vec<_>, _>>()?,
            },
            PathKind::GlobalCompact {
                gid,
                path_from_trait,
            } => PathKindOwned::GlobalCompact {
                gid: *gid,
                path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
            },
            PathKind::GlobalFull {
                gid,
                path_from_trait,
            } => PathKindOwned::GlobalFull {
                gid: gid.make_owned(),
                path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
            },
        };
        Ok(path)
    }
}

#[cfg(feature = "std")]
impl PathKindOwned {
    pub fn as_ref(&self) -> PathKind<'_> {
        match self {
            PathKindOwned::Absolute { path } => PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            },
            PathKindOwned::GlobalCompact {
                gid,
                path_from_trait,
            } => PathKind::GlobalCompact {
                gid: *gid,
                path_from_trait: RefVec::Slice {
                    slice: &path_from_trait,
                },
            },
            PathKindOwned::GlobalFull {
                gid,
                path_from_trait,
            } => PathKind::GlobalFull {
                gid: gid.as_ref(),
                path_from_trait: RefVec::Slice {
                    slice: &path_from_trait,
                },
            },
        }
    }
}

#[cfg(feature = "std")]
impl RequestKind<'_> {
    pub fn make_owned(&self) -> RequestKindOwned {
        match self {
            RequestKind::Call { args } => RequestKindOwned::Call {
                args: args.to_vec(),
            },
            RequestKind::Read => RequestKindOwned::Read,
            RequestKind::Write { data } => RequestKindOwned::Write {
                data: data.to_vec(),
            },
            RequestKind::OpenStream => RequestKindOwned::OpenStream,
            RequestKind::CloseStream => RequestKindOwned::CloseStream,
            RequestKind::Subscribe => RequestKindOwned::Subscribe,
            RequestKind::Unsubscribe => RequestKindOwned::Unsubscribe,
            RequestKind::ChangeRate { shaper_config } => RequestKindOwned::ChangeRate {
                shaper_config: *shaper_config,
            },
            RequestKind::Introspect => RequestKindOwned::Introspect,
        }
    }
}

#[cfg(feature = "std")]
impl Request<'_> {
    pub fn make_owned(&self) -> Result<RequestOwned, wire_weaver::shrink_wrap::Error> {
        Ok(RequestOwned {
            seq: self.seq,
            path_kind: self.path_kind.make_owned()?,
            kind: self.kind.make_owned(),
        })
    }
}
