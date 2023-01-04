use core::fmt::Display;
use core::fmt::Formatter;
use std::io::Error;
use vhl_stdlib::serdes::{bit_buf, buf, nibble_buf, SerializableError};

/// Error that is transferred across the wire for example in response to requests.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum XpiError {
    /// No response was received in time
    Timeout,
    /// Node reboot was detected before it was able to answer
    DeviceRebooted,
    /// Request or response wasn't fitted into memory because more important data was needing space at a time.
    PriorityLoss,
    /// Request rejected by rate shaper, even if space was available, not to exceed underlying channel bandwidth.
    /// Rejecting function calls and other non-streaming operations must be avoided.
    /// First lossy requests / subscriptions should be shaped. Then lossless (while still giving a fair
    /// chance to lossy ones) and in the latest are all other requests and responses.
    ShaperReject,
    /// When trying to access a resource that was already borrowed by someone else
    ResourceIsAlreadyBorrowed,
    /// When trying to unsubscribe twice from a resource
    AlreadyUnsubscribed,
    /// When trying to open a stream twice
    StreamIsAlreadyOpen,
    /// When trying to close a stream twice
    StreamIsAlreadyClosed,
    /// When trying to write into a const or ro property, write into stream_out or read from stream_in.
    OperationNotSupported,
    /// Returned by dispatcher if 0 len Uri is provided
    BadUri,
    /// Returned by dispatcher if trying to call a resource which is not a method
    NotAMethod,

    ReservedDiscard,
    WrongFormat,
    /// Not all nodes support 64 and 128 uri masks
    UriMaskUnsupportedType,
    /// xwfd format uses 7 bits for node addresses
    NodeIdAbove127,

    /// Unexpected internal error, reported instead of all were to be panic/unwrap/unreachable.
    Internal,
    Unimplemented,
    InternalBufError,
    InternalNibbleBufError,
    InternalBitBufError,
    InternalBbqueueError,
    ReplyBuilderError,
    IoError,

    /// Method call or property write was expecting a slice with arguments, but it wasn't provided.
    NoArgumentsProvided,

    /// Out of bounds resources array access
    OutOfBounds
}

impl SerializableError for XpiError {
    fn error_code(&self) -> u32 {
        use XpiError::*;
        match self {
            Timeout => 1, // 0 is no error
            DeviceRebooted => 2,
            PriorityLoss => 3,
            ShaperReject => 4,
            ResourceIsAlreadyBorrowed => 5,
            AlreadyUnsubscribed => 6,
            StreamIsAlreadyOpen => 7,
            StreamIsAlreadyClosed => 8,
            OperationNotSupported => 9,
            Internal => 10,
            Unimplemented => 11,
            BadUri => 12,
            NotAMethod => 13,
            NoArgumentsProvided => 14,

            ReservedDiscard => 20,
            WrongFormat => 21,
            UriMaskUnsupportedType => 22,
            NodeIdAbove127 => 23,

            InternalBufError => 31,
            InternalNibbleBufError => 32,
            InternalBitBufError => 33,
            ReplyBuilderError => 34,
            InternalBbqueueError => 35,
            IoError => 36,

            OutOfBounds => 40,
        }
    }

    fn from_error_code(value: u32) -> Option<Self> {
        use XpiError::*;
        let reason = match value {
            1 => Timeout,
            2 => DeviceRebooted,
            3 => PriorityLoss,
            4 => ShaperReject,
            5 => ResourceIsAlreadyBorrowed,
            6 => AlreadyUnsubscribed,
            7 => StreamIsAlreadyOpen,
            8 => StreamIsAlreadyClosed,
            9 => OperationNotSupported,
            10 => Internal,
            11 => Unimplemented,
            12 => BadUri,
            13 => NotAMethod,
            14 => NoArgumentsProvided,

            20 => ReservedDiscard,
            21 => WrongFormat,
            22 => UriMaskUnsupportedType,
            23 => NodeIdAbove127,

            31 => InternalBufError,
            32 => InternalNibbleBufError,
            33 => InternalBitBufError,
            34 => ReplyBuilderError,
            35 => InternalBbqueueError,
            36 => IoError,

            40 => OutOfBounds,

            _ => {
                return None;
            }
        };
        Some(reason)
    }

    fn max_code() -> u32 {
        40
    }
}

impl From<buf::Error> for XpiError {
    fn from(_: buf::Error) -> Self {
        XpiError::InternalBufError
    }
}

impl From<nibble_buf::Error> for XpiError {
    fn from(_: nibble_buf::Error) -> Self {
        XpiError::InternalNibbleBufError
    }
}

impl From<bit_buf::Error> for XpiError {
    fn from(_: bit_buf::Error) -> Self {
        XpiError::InternalBitBufError
    }
}

impl Display for XpiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for XpiError {}

#[cfg(not(feature = "no_std"))]
impl From<std::io::Error> for XpiError {
    fn from(_: Error) -> Self {
        XpiError::IoError
    }
}
