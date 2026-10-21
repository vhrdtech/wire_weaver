use shrink_wrap::{BufReader, BufWriter};

use crate::prefix::MAX_PREFIX_LEN;

pub trait Head {
    type UserKind;
    /// Longest serialized head in bytes (byte aligned), used to size buffers for stream media,
    /// where every message must fit into a frame together with its head.
    const MAX_HEAD_SIZE: usize;
    /// Smallest frame that can carry anything: the longest head plus one payload byte.
    const MIN_FRAME_SIZE: usize = Self::MAX_HEAD_SIZE + 1;

    /// Implementation details:
    /// - For [MessageKind::Full] and [MessageKind::Start], len is the total message length.
    /// - For [MessageKind::Continue] and [MessageKind::End], len is the remaining message length,
    ///   so that a receiver can skip an End whose Start was lost and continue with the rest of the frame.
    /// - [MessageKind::Full] can be re-written to [MessageKind::Start] and serialized length must not change.
    /// - [MessageKind::Continue] can be re-written to [MessageKind::End] and serialized length must not change.
    fn write(
        kind: MessageKind,
        user_kind: Self::UserKind,
        len: usize,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), WrError>;
    fn read(rd: &mut BufReader<'_>) -> Result<(MessageKind, Self::UserKind, usize), RdError>;
}

// Message bytes

/// Checksum written after message bytes (byte aligned).
///
/// A checksum covers `header` and the whole `message`. `header` is the [frame prefix](crate::prefix) of the frame
/// the checksum is written in (empty without one), followed by the head of the piece the checksum belongs to (the
/// Full, or the End of a split message), so that a frame whose prefix or head was corrupted (a user kind, a
/// length) fails it too, not only one whose payload was.
///
/// Must be of fixed length: [Checksum::read] must consume exactly [Checksum::LEN_BYTES_FULL] or
/// [Checksum::LEN_BYTES_SPLIT] bytes, so that a receiver can tell upfront whether a whole message
/// is present in a frame.
pub trait Checksum {
    /// For messages that fit fully into one frame, can be 0.
    /// No need for this checksum for media, whose frames are already checked (USB, CAN, etc.)
    const LEN_BYTES_FULL: usize;
    /// For messages that are split across one or more frames, can be 0.
    /// Theoretically can be 0 for USB as well, as it's not supposed to loose packets.
    const LEN_BYTES_SPLIT: usize;

    fn write(
        header: &[u8],
        message: &[u8],
        is_split: bool,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), WrError>;
    fn read(
        header: &[u8],
        message: &[u8],
        is_split: bool,
        rd: &mut BufReader<'_>,
    ) -> Result<(), RdError>;
}

/// Longest head a [Checksum] covers: [Head::MAX_HEAD_SIZE] must not be above it.
pub const MAX_COVERED_HEAD: usize = 8;

/// The bytes before a message a [Checksum] covers: frame prefix and head, copied out of the frame.
pub(crate) struct Header {
    bytes: [u8; MAX_PREFIX_LEN + MAX_COVERED_HEAD],
    len: usize,
}

impl Header {
    /// `prefix` followed by `head`, or nothing when `covered` is false (no checksum).
    #[inline(always)]
    pub(crate) fn new(covered: bool, prefix: &[u8], head: &[u8]) -> Self {
        let mut bytes = [0u8; MAX_PREFIX_LEN + MAX_COVERED_HEAD];
        if !covered {
            return Header { bytes, len: 0 };
        }
        bytes[..prefix.len()].copy_from_slice(prefix);
        bytes[prefix.len()..prefix.len() + head.len()].copy_from_slice(head);
        Header {
            bytes,
            len: prefix.len() + head.len(),
        }
    }

    #[inline(always)]
    pub(crate) fn get(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Tail written after checksum (byte aligned), e.g. a frame delimiter for stream media.
///
/// Must be of fixed length: [Tail::read] must consume exactly [Tail::LEN_BYTES] bytes, so that
/// a receiver can tell upfront whether a whole message is present in a frame.
pub trait Tail {
    const LEN_BYTES: usize;

    fn write(wr: &mut BufWriter<'_>) -> Result<(), WrError>;
    fn read(rd: &mut BufReader<'_>) -> Result<(), RdError>;
}

/// Start and Continue messages extend till the end of a frame. End must fit into one frame
/// together with checksum and tail, otherwise Continue is used and the rest is sent in the
/// next frame.
///
/// When frames are CRC checked and with known length (USB, CAN, etc.), knowing
/// what kind of message is ahead can prevent returning wrong ones.
/// For example if one frame is lost (not applicable to USB in theory), then the next received might contain data that
/// looks like a message.
/// Without this information and if CRC is not used on all messages only length is left.
///
/// For stream media, only Full can be returned from [Head::read] and not serialized.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Full = 0,
    Start = 1,
    Continue = 2,
    End = 3,
}

impl MessageKind {
    /// From the 2 framer bits of a head (only the lowest 2 bits of `bits` are used).
    pub(crate) fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0 => MessageKind::Full,
            1 => MessageKind::Start,
            2 => MessageKind::Continue,
            _ => MessageKind::End,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrError {
    OutOfBounds,
    TooBig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdError {
    NeedMoreData,
    BadHead,
    BadTail,
    BadLength,
    ChecksumMismatch,
}

pub struct NopChecksum;

impl Checksum for NopChecksum {
    const LEN_BYTES_FULL: usize = 0;
    const LEN_BYTES_SPLIT: usize = 0;

    fn write(
        _header: &[u8],
        _message: &[u8],
        _is_split: bool,
        _wr: &mut BufWriter<'_>,
    ) -> Result<(), WrError> {
        Ok(())
    }

    fn read(
        _header: &[u8],
        _message: &[u8],
        _is_split: bool,
        _rd: &mut BufReader<'_>,
    ) -> Result<(), RdError> {
        Ok(())
    }
}

pub struct NopTail;

impl Tail for NopTail {
    const LEN_BYTES: usize = 0;

    fn write(_wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        Ok(())
    }

    fn read(_rd: &mut BufReader<'_>) -> Result<(), RdError> {
        Ok(())
    }
}

/// 1 byte tail
pub struct ByteTail<const BYTE: u8>;

impl<const BYTE: u8> Tail for ByteTail<BYTE> {
    const LEN_BYTES: usize = 1;

    fn write(wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        wr.write_u8(BYTE)?;
        Ok(())
    }

    fn read(rd: &mut BufReader<'_>) -> Result<(), RdError> {
        if rd.read_u8()? == BYTE {
            Ok(())
        } else {
            Err(RdError::BadTail)
        }
    }
}

impl From<shrink_wrap::Error> for WrError {
    fn from(_: shrink_wrap::Error) -> Self {
        WrError::OutOfBounds
    }
}

impl From<shrink_wrap::Error> for RdError {
    fn from(_: shrink_wrap::Error) -> Self {
        RdError::NeedMoreData
    }
}
