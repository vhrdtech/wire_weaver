// use thiserror::Error;
use crate::serdes::{nibble_buf, bit_buf, NibbleBufMut, DeserializeVlu4, NibbleBuf};
use crate::serdes::traits::SerializeVlu4;
use crate::serdes::vlu4::vlu32::Vlu32;
use crate::serdes::nibble_buf::Error as NibbleBufError;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    /// Returned by dispatcher if 0 len Uri is provided
    BadUri,
    /// Returned by dispatcher if trying to call a resource which is not a method
    NotAMethod,

    /// Unexpected internal error, reported instead of all were to be panic/unwrap/unreachable.
    Internal,
    InternalBufError,
    InternalNibbleBufError,
    InternalBitBufError,
    ReplyBuilderError,

    /// Method call or property write was expecting a slice with arguments, but it wasn't provided.
    NoArgumentsProvided,
}

impl FailReason {
    pub fn from_u32(value: u32) -> Self {
        use FailReason::*;
        match value {
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
            11 => BadUri,
            12 => NotAMethod,
            13 => InternalBufError,
            14 => InternalNibbleBufError,
            15 => InternalBitBufError,
            16 => ReplyBuilderError,
            17 => NoArgumentsProvided,
            _ => Internal
        }
    }

    pub fn to_u32(&self) -> u32 {
        use FailReason::*;
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
            BadUri => 11,
            NotAMethod => 12,
            InternalBufError => 13,
            InternalNibbleBufError => 14,
            InternalBitBufError => 15,
            ReplyBuilderError => 16,
            NoArgumentsProvided => 17,
        }
    }
}

impl From<crate::serdes::buf::Error> for FailReason {
    fn from(_: crate::serdes::buf::Error) -> Self {
        FailReason::InternalBufError
    }
}

impl From<crate::serdes::nibble_buf::Error> for FailReason {
    fn from(_: crate::serdes::nibble_buf::Error) -> Self {
        FailReason::InternalNibbleBufError
    }
}

impl From<crate::serdes::bit_buf::Error> for FailReason {
    fn from(_: crate::serdes::bit_buf::Error) -> Self {
        FailReason::InternalBitBufError
    }
}

impl From<u32> for FailReason {
    fn from(e: u32) -> Self {
        FailReason::from_u32(e)
    }
}

// impl Into<Vlu32> for FailReason {
//     fn into(self) -> Vlu32 {
//         Vlu32(self.to_u32())
//     }
// }

impl SerializeVlu4 for FailReason {
    type Error = NibbleBufError;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        wgr.put_vlu4_u32(self.to_u32())?;
        Ok(())
    }

    fn len_nibbles(&self) -> usize {
        Vlu32(self.to_u32()).len_nibbles()
    }
}

impl<'i> DeserializeVlu4<'i> for Result<(), FailReason> {
    type Error = crate::serdes::nibble_buf::Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let code = rdr.get_vlu4_u32()?;
        if code == 0 {
            Ok(Ok(()))
        } else {
            Ok(Err(FailReason::from_u32(code)))
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum XpiVlu4Error {
    // #[error("Nibble buf reader error")]
    NibbleBuf(nibble_buf::Error),
    // #[error("Bit buf reader error")]
    BitBuf(bit_buf::Error),
    //
    Vlu4SliceArray,
    // #[error("Unreachable reached")]
    InternalError,
    // #[error("Reserved uri mask type")]
    UriMaskReserved,
    // #[error("Unsupported uri mask type")]
    UriMaskUnsupportedType,

    // #[error("Expected request")]
    NotARequest,
    NotAResponse,
    // #[error("Unsupported reserved value, not ignorable.")]
    ReservedDiscard,

    // #[error("Feature is not yet implemented")]
    Unimplemented,
    NodeId
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