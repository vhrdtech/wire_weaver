// use thiserror::Error;
use crate::serdes::{nibble_buf, bit_buf, NibbleBufMut};
use crate::serdes::traits::SerializeVlu4;

#[derive(Copy, Clone, Debug)]
pub enum FailReason {
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
}

impl SerializeVlu4 for FailReason {
    type Error = XpiVlu4Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        let discriminant = match self {
            FailReason::Timeout => 1, // 0 is no error
            FailReason::DeviceRebooted => 2,
            FailReason::PriorityLoss => 3,
            FailReason::ShaperReject => 4,
            FailReason::ResourceIsAlreadyBorrowed => 5,
            FailReason::AlreadyUnsubscribed => 6,
            FailReason::StreamIsAlreadyOpen => 7,
            FailReason::StreamIsAlreadyClosed => 8,
            FailReason::OperationNotSupported => 9,
        };
        wgr.put_vlu4_u32(discriminant)?;
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum XpiVlu4Error {
    // #[error("Nibble buf reader error")]
    NibbleBuf(nibble_buf::Error),
    // #[error("Bit buf reader error")]
    BitBuf(bit_buf::Error),
    // #[error("Unreachable reached")]
    InternalError,
    // #[error("Reserved uri mask type")]
    UriMaskReserved,
    // #[error("Unsupported uri mask type")]
    UriMaskUnsupportedType,

    // #[error("Expected request")]
    NotARequest,
    // #[error("Unsupported reserved value, not ignorable.")]
    ReservedDiscard,

    // #[error("Feature is not yet implemented")]
    Unimplemented
}

impl From<nibble_buf::Error> for XpiVlu4Error {
    fn from(e: nibble_buf::Error) -> Self {
        XpiVlu4Error::NibbleBuf(e)
    }
}

impl From<bit_buf::Error> for XpiVlu4Error {
    fn from(e: bit_buf::Error) -> Self {
        XpiVlu4Error::BitBuf(e)
    }
}