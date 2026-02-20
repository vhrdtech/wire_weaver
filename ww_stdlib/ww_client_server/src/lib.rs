#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]

pub mod util;

use wire_weaver::prelude::*;
use ww_version::{CompactVersion, FullVersion};

#[cfg(feature = "std")]
use ww_version::FullVersionOwned;

/// Version of this protocol itself, exchanged and checked during the link setup phase with a remote device.
pub const FULL_VERSION: FullVersion = full_version!();

/// Operation (call, read, write, etc.) to be performed on a resource together with a request ID and resource path.
#[derive_shrink_wrap]
#[owned = "std"]
#[derive(Debug)]
pub struct Request<'i> {
    /// Request ID, starting from 1 and wrapping back to 1 that allows to map responses to requests.
    /// 0 means no answer is expected.
    pub seq: u16,

    /// Specifies whether a resource is addressed explicitly, using a full path to it or through global trait ID.
    pub path_kind: PathKind<'i>,

    /// Action being requested
    pub kind: RequestKind<'i>,
}

/// Path to a resource.
/// 3 modes of addressing is supported:
/// * Absolute - only the number path to a resource is used, smallest size
/// * GlobalCompact - request to a trait resource, for commonly used traits that are used often and have an ID assigned
/// * GlobalFull - request to a trait resource defined in an arbitrary Rust crate, full crate name, and it's version is used as an ID
///
/// [Global ID registry](https://github.com/vhrdtech/ww_stdlib/tree/main/ww_global)
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[owned = "std"]
#[derive(Debug, Clone)]
pub enum PathKind<'i> {
    /// Full path to a resource, regardless whether it is in a trait or not.
    Absolute { path: RefVec<'i, UNib32> },

    /// Request is for a trait implemented at root level, through a global unique ID (manually assigned for common traits).
    GlobalCompact {
        /// CompactVersion consists of 3 UNib32's, so the smallest additional size of such call if all numbers are <= 7 is 2 bytes.
        gid: CompactVersion,
        /// Used to identify a trait resource, starting from trait root.
        path_from_trait: RefVec<'i, UNib32>,
    },

    /// Request is for a trait implemented at root level, through a crates.io name and version (any crate can be used as is).
    /// This kind of request is the biggest, because full crate name is used.
    GlobalFull {
        gid: FullVersion<'i>,
        /// Used to identify a trait resource, starting from trait root.
        path_from_trait: RefVec<'i, UNib32>,
    },
}

/// Operation (call, read, write, etc.) to be performed on a resource.
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
pub enum RequestKind<'i> {
    /// Call a method with provided arguments.
    /// Expected to get EventKind::ReturnValue, unless request ID is 0.
    Call {
        /// Arguments are put into a struct and serialized using shrink_wrap to obtain this byte array.
        args: RefVec<'i, u8>,
    },

    /// Read a property.
    /// Expected to get EventKind::ReadValue with property bytes.
    Read,

    // Read the default value of a property, if available.
    // ReadDefault,

    // Read multiple properties at once, using a list of paths or a glob pattern.
    // ReadMany,
    /// Write property or stream down. Property value is serialized fully into a byte array using shrink_wrap.
    /// Objects of a stream are also serialized in full and sent as one unit.
    Write { data: RefVec<'i, u8> },

    // Write default value (if available) to a property, without sending any data.
    // WriteDefault,

    // Write multiple properties at once, using a list of paths or a glob pattern?.
    // WriteMany,
    /// Subscribe to property changes
    Subscribe,
    /// Unsubscribe from property changes
    Unsubscribe,

    /// Set a limit on how often property or stream updates are sent. Optional.
    ChangeRate { shaper_config: ShaperConfig },

    /// Stream sideband channel (open, close, frame sync, etc.). Optional to use.
    StreamSideband { sideband_cmd: StreamSidebandCommand },

    /// Send serialized AST describing a resource and all related types, see `ww_self` for format.
    /// Optional, for simplicity can be implemented only at root level, sending all API tree.
    Introspect,
    // Version,
    // Borrow,
    // Release,
    // Heartbeat,
}

/// Sideband command for a stream, delivered in the same order with stream data.
/// Optional, user can choose to send stream updates without using the sideband channel.
/// This is a separate enum to make generated code more convenient - only one function for user to implement.
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[derive(Debug, Copy, Clone)]
pub enum StreamSidebandCommand {
    Open,
    Close,
    /// If stream is a sequence of bytes, can be used to delimit frames
    FrameSync,
    ChangeRate(ShaperConfig),
    SizeHint(u32),
    User(u32),
}

/// Asynchronous result with a request ID, sent back from server to client, as a response to a Request or on stream or properties updates.
#[derive_shrink_wrap]
#[owned = "std"]
#[derive(Debug)]
pub struct Event<'i> {
    /// Same event ID from Request.
    /// 0 for stream data updates.
    pub seq: u16,
    /// Request can be wrong or unsupported, in which case an error is sent back.
    pub result: Result<EventKind<'i>, Error<'i>>,
}

/// Asynchronous event, sent back from server to client, as a response to a Request or on stream or properties updates.
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
pub enum EventKind<'i> {
    /// Sent in response to RequestKind::Call, unless request ID is 0.
    ReturnValue {
        /// Serialized return value of a method.
        /// If user-defined type is used, it is serialized directly. If one of the built-in types is used, it is put into a
        /// struct and that struct is serialized instead.
        data: RefVec<'i, u8>,
    },

    /// Send in response to RequestKind::Read.
    ReadValue {
        /// Serialized property value.
        data: RefVec<'i, u8>,
    },

    /// Sent in response to RequestKind::Write, only for properties and when request ID is not 0.
    Written,

    /// Sent by user code whenever stream have more data or whenever applicable.
    StreamData {
        /// When subscribing through trait interface, this path is used later to match stream updates to an original request.
        path: RefVec<'i, UNib32>,
        /// Stream data, can be a whole frame or a chunk of a byte stream.
        data: RefVec<'i, u8>,
    },
    /// Optionally sent by user in response to RequestKind::StreamSideband or whenever applicable.
    StreamSideband {
        /// When subscribing through trait interface, this path is used later to match stream updates to an original request.
        path: RefVec<'i, UNib32>,
        sideband_event: StreamSidebandEvent,
    },

    /// Sent in response to RequestKind::Subscribe for properties. Optional.
    Subscribed {
        /// When subscribing through trait interface, this path is used later to match stream updates to an original request.
        path: RefVec<'i, UNib32>,
    },
    /// Sent in response to RequestKind::Unsubscribe for properties. Optional.
    Unsubscribed { path: RefVec<'i, UNib32> },

    /// Sent in response to RequestKind::ChangeRata for properties. Optional.
    RateChanged,

    /// Sent in response to [RequestKind::Introspect] potentially in multiple chunks.
    Introspect { ww_self_bytes_chunk: RefVec<'i, u8> },
}

/// Stream sideband event, sent in response to StreamSidebandCommand or asynchronously.
/// Optional, user can choose to send stream updates without using the sideband channel.
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[final_structure]
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum StreamSidebandEvent {
    /// Sent if a stream was successfully opened
    Opened,
    /// Sent if a stream was successfully closed
    Closed,
    /// If a stream is a sequence of bytes, can be used to delimit frames
    FrameSync,
    /// Can be used to indicate total size of the upcoming stream updates
    SizeHint(u32),
    /// User event, can be used to indicate errors or other data
    User(u32),
}

#[derive_shrink_wrap]
#[derive(Debug)]
#[owned = "std"]
pub struct Error<'i> {
    /// Unique error ID for each error in generated code. Can be used to map an error back to source code.
    err_seq: u32,
    /// Actual error kind.
    kind: ErrorKind<'i>,
}

/// Various errors that can occur during Request processing.
/// TODO: Add shrink_wrap error here as well for more context
#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[derive(Debug)]
#[owned = "std"]
pub enum ErrorKind<'i> {
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

    /// Failed to deserialize arguments
    ArgsDesFailed,
    /// Failed to deserialize path
    PathDesFailed,
    /// Failed to deserialize property value
    PropertyDesFailed,
    /// Failed to serialize response
    ResponseSerFailed,
    /// Request is good, but requested operation is not yet implemented
    OperationNotImplemented,
    /// Tried to read a property with request seq number set to 0, meaning no response is expected
    ReadPropertyWithSeqZero,
    /// Returned if only absolute paths are handled (on very resource constrained nodes)
    PathKindNotSupported,
    /// Forwarded user error in serialized form
    UserBytes(RefVec<'i, u8>),
}

/// Optional shaper configuration request.
#[derive_shrink_wrap]
#[ww_repr(u4)]
#[derive(Debug, Copy, Clone)]
pub enum ShaperConfig {
    NoLimit,
    MaxBitrate { bytes_per_s: u32 },
    MaxRate { events_per_s: u32 },
}

impl PathKind<'_> {
    pub fn absolute(path_from_root: &[UNib32]) -> PathKind<'_> {
        PathKind::Absolute {
            path: RefVec::Slice {
                slice: path_from_root,
            },
        }
    }

    pub fn global<'a>(
        global_full: FullVersion<'static>,
        global_compact: Option<CompactVersion>,
        path_from_trait: &'a [UNib32],
    ) -> PathKind<'a> {
        if let Some(compact) = global_compact {
            PathKind::GlobalCompact {
                gid: compact,
                path_from_trait: RefVec::Slice {
                    slice: path_from_trait,
                },
            }
        } else {
            PathKind::GlobalFull {
                gid: global_full,
                path_from_trait: RefVec::Slice {
                    slice: path_from_trait,
                },
            }
        }
    }
}

impl<'i> Error<'i> {
    pub fn new(err_seq: u32, kind: ErrorKind<'i>) -> Error<'i> {
        Self { err_seq, kind }
    }

    pub fn not_supported(err_seq: u32) -> Self {
        Self {
            err_seq,
            kind: ErrorKind::OperationNotSupported,
        }
    }

    pub fn bad_path(err_seq: u32) -> Self {
        Self {
            err_seq,
            kind: ErrorKind::BadPath,
        }
    }

    pub fn response_ser_failed(err_seq: u32) -> Self {
        Self {
            err_seq,
            kind: ErrorKind::ResponseSerFailed,
        }
    }
}

#[cfg(feature = "std")]
impl Error<'_> {
    pub fn make_owned(&self) -> ErrorOwned {
        let kind = match &self.kind {
            ErrorKind::OperationNotSupported => ErrorKindOwned::OperationNotSupported,
            ErrorKind::BadPath => ErrorKindOwned::BadPath,
            ErrorKind::BadIndex => ErrorKindOwned::BadIndex,
            ErrorKind::ExpectedArrayIndexGotNone => ErrorKindOwned::ExpectedArrayIndexGotNone,
            ErrorKind::ArrayIndexDesFailed => ErrorKindOwned::ArrayIndexDesFailed,
            ErrorKind::ArgsDesFailed => ErrorKindOwned::ArgsDesFailed,
            ErrorKind::PathDesFailed => ErrorKindOwned::PathDesFailed,
            ErrorKind::PropertyDesFailed => ErrorKindOwned::PropertyDesFailed,
            ErrorKind::ResponseSerFailed => ErrorKindOwned::ResponseSerFailed,
            ErrorKind::OperationNotImplemented => ErrorKindOwned::OperationNotImplemented,
            ErrorKind::ReadPropertyWithSeqZero => ErrorKindOwned::ReadPropertyWithSeqZero,
            ErrorKind::PathKindNotSupported => ErrorKindOwned::PathKindNotSupported,
            ErrorKind::UserBytes(bytes) => ErrorKindOwned::UserBytes(bytes.to_vec()),
        };
        ErrorOwned {
            err_seq: self.err_seq,
            kind,
        }
    }
}

#[cfg(feature = "std")]
impl PathKind<'_> {
    pub fn make_owned(&self) -> Result<PathKindOwned, shrink_wrap::Error> {
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
                path: RefVec::Slice { slice: path },
            },
            PathKindOwned::GlobalCompact {
                gid,
                path_from_trait,
            } => PathKind::GlobalCompact {
                gid: *gid,
                path_from_trait: RefVec::Slice {
                    slice: path_from_trait,
                },
            },
            PathKindOwned::GlobalFull {
                gid,
                path_from_trait,
            } => PathKind::GlobalFull {
                gid: gid.as_ref(),
                path_from_trait: RefVec::Slice {
                    slice: path_from_trait,
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
            RequestKind::StreamSideband { sideband_cmd } => RequestKindOwned::StreamSideband {
                sideband_cmd: *sideband_cmd,
            },
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
    pub fn make_owned(&self) -> Result<RequestOwned, shrink_wrap::Error> {
        Ok(RequestOwned {
            seq: self.seq,
            path_kind: self.path_kind.make_owned()?,
            kind: self.kind.make_owned(),
        })
    }
}
