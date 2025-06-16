#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_version::{FullVersion, Version};

#[cfg(feature = "std")]
use ww_version::VersionOwned;

pub const PROTOCOL_GID: u32 = 1; // TODO: Remove!

pub const FULL_VERSION: FullVersion = wire_weaver::full_version!();

#[derive_shrink_wrap]
#[shrink_wrap(no_alloc)]
#[owned = "std"]
#[derive(Debug)]
struct Request<'i> {
    /// If 0 - no answer is expected
    pub seq: u16,
    pub path: RefVec<'i, UNib32>,
    pub kind: RequestKind<'i>,
}

#[derive_shrink_wrap]
#[shrink_wrap(no_alloc)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
enum RequestKind<'i> {
    Version,
    Call {
        args: RefVec<'i, u8>,
    },
    // CallTraitMethod { trait_gid: Either<FullVersion, CompactVersion>, path: RefVec<'i, UNib32>, args: Vec<u8> },
    /// Read property
    Read,
    // ReadDefault,
    // ReadMany,
    /// Write property or stream down
    Write {
        data: RefVec<'i, u8>,
    },
    // WriteDefault,
    // WriteMany,
    OpenStream,
    CloseStream,
    ChangeRate {
        shaper_config: ShaperConfig,
    },
    /// Subscribe to property changes
    Subscribe,
    /// Unsubscribe from property changes
    Unsubscribe,
    // Borrow,
    // Release,
    Introspect,
    // Heartbeat,
}

#[derive_shrink_wrap]
#[shrink_wrap(no_alloc)]
#[owned = "std"]
#[derive(Debug)]
struct Event<'i> {
    pub seq: u16,
    // path
    pub result: Result<EventKind<'i>, Error>,
}

#[derive_shrink_wrap]
#[shrink_wrap(no_alloc)]
#[final_structure]
#[owned = "std"]
#[derive(Debug)]
enum EventKind<'i> {
    Version {
        protocol_id: u32,
        version: Version<'i>,
    },
    ReturnValue {
        data: RefVec<'i, u8>,
    },
    ReadValue {
        data: RefVec<'i, u8>,
    },
    Written,
    StreamOpened,
    // If stream is a sequence of bytes, can be used to delimit frames or send other data out of band
    // StreamDelimiter { path: Vec<nib16>, user_data: u8 },
    // TODO: Add Option<SizeHint>
    StreamUpdate {
        path: Vec<UNib32>,
        data: RefVec<'i, u8>,
    },
    StreamClosed,
    Subscribed,
    RateChanged,
    Unsubscribed,
    // Borrowed,
    // Released,
    Introspect {
        ww_bytes: RefVec<'i, u8>,
    },
    // Heartbeat { data: Vec<u8> },
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
#[self_describing]
#[derive(Debug)]
enum Error {
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
}

#[derive_shrink_wrap]
#[derive(Debug)]
enum ShaperConfig {
    NoLimit,
    MaxBitrate { byte_per_s: u32 },
    MaxRate { events_per_s: u32 },
}
