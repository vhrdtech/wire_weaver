#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]

pub mod builder;
pub mod multi_req;
pub mod util;

use wire_weaver::{prelude::*, shrink_wrap::tail_bytes::TailBytes};
use ww_version::{CompactVersion, FullVersion};

#[cfg(feature = "std")]
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
#[cfg(feature = "std")]
use ww_version::FullVersionOwned;

/// Version of this protocol itself, exchanged and checked during the link setup phase with a remote device.
pub const FULL_VERSION: FullVersion = full_version!();
pub const COMPACT_VERSION: CompactVersion = CompactVersion::new(
    ww_global::WW_CLIENT_SERVER,
    FULL_VERSION.version.major.0,
    FULL_VERSION.version.minor.0,
    FULL_VERSION.version.patch.0,
);

/// Operation (call, read, write, etc.) to be performed on a resource together with a request ID and resource path.
///
/// Smallest size:
/// - 4B (seq, empty Absolute path - root req, Kind with no data)
/// - 5B (seq, path len 1 <= 7, Kind with 1B args)
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug))]
pub struct Request<'i> {
    /// Request ID, starting from 1 and wrapping back to 1 that allows to map responses to requests.
    /// 0 means no answer is expected.
    pub seq: u16,

    /// Specifies whether a resource is addressed explicitly, using a full path to it or through global trait ID.
    pub path_kind: PathKind<'i>,

    /// Action being requested
    pub kind: RequestKind<'i>,
}

/// Request sequence number.
/// Serialized as 1 byte if <= 127, 2 bytes if <= 16384
#[allow(dead_code)]
#[derive(Debug)]
pub struct Seq(u32);

/// Path to a resource.
/// 3 modes of addressing are supported:
/// * Absolute - only the number path to a resource is used, the smallest size
/// * GlobalCompact - request to a trait resource, for commonly used traits that are used often and have an ID assigned
/// * GlobalFull - request to a trait resource defined in an arbitrary Rust crate, full crate name, and its version is used as an ID
///
/// [Global ID registry](https://github.com/vhrdtech/ww_stdlib/tree/main/ww_global)
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, Clone), final_structure, ww_repr = nib)]
pub enum PathKind<'i> {
    /// Full path to a resource, regardless of whether it is in a trait or not.
    Absolute { path: RefVec<'i, UNib32> },

    /// Request is for a trait implemented at root level, through a global unique ID (manually assigned for common traits).
    GlobalCompact {
        /// CompactVersion consists of 3 UNib32's, so the smallest additional size of such a call if all numbers are <= 7 is 2 bytes.
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
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug), final_structure, ww_repr = nib)]
pub enum RequestKind<'i> {
    /// Call a method with provided arguments.
    /// Expected to get [EventKind::Value], unless request ID is 0.
    Call {
        /// Arguments are put into a struct and serialized using shrink_wrap to obtain this byte array.
        args: TailBytes<'i>,
    },
    /// Call the same method over an array of traits or several methods in one request.
    MultiCall {
        /// List of resources to call
        multi_idx: MultiIndex<'i>,
        /// Some when calling the same method on an array of traits
        in_each_array_id: Option<UNib32>,
        multi_args: MultiArgs<'i>,
    },

    /// Read a property.
    /// Expected to get [EventKind::Value] with property bytes.
    Read,
    /// Read the same property over an array of traits or several properties in one request.
    MultiRead {
        /// List of properties to read from
        multi_idx: MultiIndex<'i>,
        /// Some when reading the same property on an array of traits
        in_each_array_id: Option<UNib32>,
    },

    /// Write property or stream down. Property value is serialized fully into a byte array using shrink_wrap.
    /// Objects of a stream are also serialized in full and sent as one unit.
    Write { data: TailBytes<'i> },
    /// Write multiple properties or streams in one request.
    MultiWrite {
        /// List of resources to write to
        multi_idx: MultiIndex<'i>,
        /// Some when addressing the same resource on an array of traits
        in_each_array_id: Option<UNib32>,
        multi_data: MultiArgs<'i>,
    },

    /// Stream sideband channel (open, close, frame sync, etc.). Optional to use.
    StreamSideband { sideband: StreamSideband },

    // Write default value (if available) to a property, without sending any data.
    // WriteDefault,
    /// Send serialized AST describing a resource and all related types, see `ww_self` for format.
    /// Optional, for simplicity can be implemented only at root level, sending all API tree.
    Introspect,
}

/// Index for a multi request. Two kinds of multi requests are possible:
///
/// ```
/// #[ww_trait]
/// trait GpioBank {
///     ww_impl!(pin[]: Pin);
/// }
///
/// #[ww_trait]
/// trait Pin {
///     fn set_level(is_high: bool);
///     fn set_mode(mode: Mode);
/// }
/// ```
///
/// # Same resource in an array
/// Can make a MultiCall:: request to '/0' (array itself) with MultiIndex::Range(0..10).
/// To call set_level() on pins 0 to 10 in one request.
///
/// # Different resources at one API level
/// Can make a MultiCall:: request to '0/3' (third pin in the array) with MultiIndex::List(0, 1).
/// To call set_level(args0) and then set_mode(args1) in one request.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug), ww_repr = u2)]
pub enum MultiIndex<'i> {
    // All,
    Range(Range<UNib32>),
    List(RefVec<'i, UNib32>),
    Mask32(u32),
}

/// Serialized arguments / property or stream data for a multi-request.
/// Same can be used when all arguments are equal (e.g., calling set_mode(Output) for multiple pins).
///
/// `Different` reuses a BufReader, while `Same` resets it to the beginning before processing a request.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug), ww_repr = u1)]
pub enum MultiArgs<'i> {
    Same(RefVec<'i, u8>),
    Different(RefVec<'i, u8>),
}

/// Asynchronous result with a request ID, sent back from server to client, as a response to a Request or on stream or properties updates.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug))]
pub struct Event<'i> {
    /// Same event ID from Request.
    /// 0 for stream data updates.
    pub seq: u16,
    /// Request can be wrong or unsupported, in which case an error is sent back.
    pub result: Result<EventKind<'i>, Error<'i>>,
}

/// Asynchronous event, sent back from server to client, as a response to a Request or on stream or properties updates.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Debug),
    final_structure,
    ww_repr = nib,
    discriminants
)]
pub enum EventKind<'i> {
    /// Sent in response to [RequestKind::Call] or [RequestKind::Read], unless request ID is 0.
    Value {
        /// Serialized return value of a method.
        // TODO: add is_multipart: bool, is_end: bool or enum Kind { SinglePart, MultiPart, MultiPartEnd(crc) }?
        // TODO: add CRC?
        data: TailBytes<'i>,
    },

    /// Sent in response to RequestKind::Write, only for properties and when request ID is not 0.
    Written,

    /// Sent by user code whenever a stream has more data or whenever applicable.
    StreamData {
        /// When subscribing through trait interface, this path is used later to match stream updates to an original request.
        path: RefVec<'i, UNib32>,
        /// Stream data can be a whole frame or a chunk of a byte stream.
        data: TailBytes<'i>,
    },

    /// Optionally sent by in response to RequestKind::StreamSideband or whenever applicable.
    StreamSideband {
        /// When subscribing through trait interface, this path is used later to match stream updates to an original request.
        path: RefVec<'i, UNib32>,
        sideband: StreamSideband,
    },
}

/// Stream sideband event, sent in response to StreamSidebandCommand or asynchronously.
/// Optional, user can choose to send stream updates without using the sideband channel.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(PartialEq, Eq, Debug, Copy, Clone),
    final_structure,
    ww_repr = nib
)]
pub enum StreamSideband {
    /// Sent if a stream was successfully opened
    Open,
    /// Sent if a stream was successfully closed
    Close,
    /// If a stream is a sequence of bytes, can be used to delimit frames
    FrameSync,
    /// Can be used to indicate total size of the upcoming stream updates
    SizeHint(UNib32),
    /// User event, can be used to indicate errors or other data
    User(UNib32),
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug), final_structure)]
pub struct Error<'i> {
    /// Unique error ID for each error in generated code. Can be used to map an error back to source code.
    err_seq: UNib32,
    /// Actual error kind.
    kind: ErrorKind<'i>,
}

/// Various errors that can occur during Request processing.
/// TODO: Add shrink_wrap error here as well for more context
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug), ww_repr = u8, discriminants)]
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
    Unimplemented,
    /// Tried to read a property with request seq number set to 0, meaning no response is expected
    ReadPropertyWithSeqZero,
    /// Returned if only absolute paths are handled (on very resource constrained nodes)
    PathKindNotSupported,
    /// Forwarded user error in serialized form
    UserBytes(RefVec<'i, u8>),
    /// Forwarded user error
    UserStr(&'i str),
}

// Optional shaper configuration request.
// #[derive_shrink_wrap]
// #[ww_repr(nib)]
// #[derive(Debug, Copy, Clone)]
// pub enum ShaperConfig {
//     NoLimit,
//     MaxBitrate { bytes_per_s: u32 },
//     MaxRate { events_per_s: u32 },
// }

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
        Self {
            err_seq: UNib32(err_seq),
            kind,
        }
    }

    pub fn not_supported(err_seq: u32) -> Self {
        Self {
            err_seq: UNib32(err_seq),
            kind: ErrorKind::OperationNotSupported,
        }
    }

    pub fn bad_path(err_seq: u32) -> Self {
        Self {
            err_seq: UNib32(err_seq),
            kind: ErrorKind::BadPath,
        }
    }

    pub fn response_ser_failed(err_seq: u32) -> Self {
        Self {
            err_seq: UNib32(err_seq),
            kind: ErrorKind::ResponseSerFailed,
        }
    }

    pub fn unimplemented(err_seq: u32) -> Self {
        Self {
            err_seq: UNib32(err_seq),
            kind: ErrorKind::Unimplemented,
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
            ErrorKind::Unimplemented => ErrorKindOwned::Unimplemented,
            ErrorKind::ReadPropertyWithSeqZero => ErrorKindOwned::ReadPropertyWithSeqZero,
            ErrorKind::PathKindNotSupported => ErrorKindOwned::PathKindNotSupported,
            ErrorKind::UserBytes(bytes) => ErrorKindOwned::UserBytes(bytes.to_vec()),
            ErrorKind::UserStr(s) => ErrorKindOwned::UserStr(s.to_string()),
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

impl RequestKind<'_> {
    pub fn discriminants(&self) -> EventKindDiscriminants {
        match self {
            RequestKind::Call { .. } => EventKindDiscriminants::Value,
            RequestKind::MultiCall { .. } => EventKindDiscriminants::Value,
            RequestKind::Read => EventKindDiscriminants::Value,
            RequestKind::MultiRead { .. } => EventKindDiscriminants::Value,
            RequestKind::Write { .. } => EventKindDiscriminants::Written,
            RequestKind::MultiWrite { .. } => EventKindDiscriminants::Written,
            RequestKind::StreamSideband { .. } => EventKindDiscriminants::StreamSideband,
            RequestKind::Introspect => EventKindDiscriminants::StreamData,
        }
    }
}

#[cfg(feature = "std")]
impl RequestKind<'_> {
    pub fn make_owned(&self) -> Result<RequestKindOwned, shrink_wrap::Error> {
        let req = match self {
            RequestKind::Call { args } => RequestKindOwned::Call {
                args: args.make_owned(),
            },
            RequestKind::MultiCall {
                multi_idx,
                in_each_array_id,
                multi_args,
            } => RequestKindOwned::MultiCall {
                multi_idx: multi_idx.make_owned()?,
                in_each_array_id: *in_each_array_id,
                multi_args: multi_args.make_owned()?,
            },
            RequestKind::Read => RequestKindOwned::Read,
            RequestKind::MultiRead {
                multi_idx,
                in_each_array_id,
            } => RequestKindOwned::MultiRead {
                multi_idx: multi_idx.make_owned()?,
                in_each_array_id: *in_each_array_id,
            },
            RequestKind::Write { data } => RequestKindOwned::Write {
                data: data.make_owned(),
            },
            RequestKind::MultiWrite {
                multi_idx,
                in_each_array_id,
                multi_data,
            } => RequestKindOwned::MultiWrite {
                multi_idx: multi_idx.make_owned()?,
                in_each_array_id: *in_each_array_id,
                multi_data: multi_data.make_owned()?,
            },
            RequestKind::StreamSideband { sideband } => RequestKindOwned::StreamSideband {
                sideband: *sideband,
            },
            RequestKind::Introspect => RequestKindOwned::Introspect,
        };
        Ok(req)
    }
}

impl Request<'_> {
    pub fn set_seq(bytes: &mut [u8], seq: u16) {
        let seq_le = seq.to_le_bytes();
        bytes[0] = seq_le[0];
        bytes[1] = seq_le[1];
    }
}

#[cfg(feature = "std")]
impl Request<'_> {
    pub fn make_owned(&self) -> Result<RequestOwned, shrink_wrap::Error> {
        Ok(RequestOwned {
            seq: self.seq,
            path_kind: self.path_kind.make_owned()?,
            kind: self.kind.make_owned()?,
        })
    }
}

#[cfg(feature = "std")]
impl MultiArgs<'_> {
    pub fn make_owned(&self) -> Result<MultiArgsOwned, shrink_wrap::Error> {
        match self {
            MultiArgs::Same(args) => Ok(MultiArgsOwned::Same(args.to_vec())),
            MultiArgs::Different(args) => Ok(MultiArgsOwned::Different(args.to_vec())),
        }
    }
}

#[cfg(feature = "std")]
impl MultiIndex<'_> {
    pub fn make_owned(&self) -> Result<MultiIndexOwned, shrink_wrap::Error> {
        match self {
            // MultiIndex::All => Ok(MultiIndexOwned::All),
            MultiIndex::Range(r) => Ok(MultiIndexOwned::Range(r.clone())),
            MultiIndex::List(list) => Ok(MultiIndexOwned::List(
                list.iter().collect::<Result<Vec<_>, _>>()?,
            )),
            MultiIndex::Mask32(mask) => Ok(MultiIndexOwned::Mask32(*mask)),
        }
    }
}
