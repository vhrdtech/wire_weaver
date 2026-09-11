use shrink_wrap::{BufReader, BufWriter};

pub trait Head {
    type UserKind;
    const MIN_FRAME_SIZE: usize;

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

pub trait Checksum {
    /// For messages that fit fully into one frame, can be 0.
    /// No need for this checksum for media, whose frames are already checked (USB, CAN, etc.)
    const LEN_BYTES_FULL: usize;
    /// For messages that are split across one or more frames, can be 0.
    /// Theoretically can be 0 for USB as well, as it's not supposed to loose packets.
    const LEN_BYTES_SPLIT: usize;

    fn write(message: &[u8], is_split: bool, wr: &mut BufWriter<'_>) -> Result<(), WrError>;
    fn read(message: &[u8], is_split: bool, rd: &mut BufReader<'_>) -> Result<(), RdError>;
}

pub trait Tail {
    const LEN_BYTES: usize;

    fn write(wr: &mut BufWriter<'_>) -> Result<(), WrError>;
    fn read(rd: &mut BufReader<'_>) -> Result<(), RdError>;
}

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

    fn write(_message: &[u8], _is_split: bool, _wr: &mut BufWriter<'_>) -> Result<(), WrError> {
        Ok(())
    }

    fn read(_message: &[u8], _is_split: bool, _rd: &mut BufReader<'_>) -> Result<(), RdError> {
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
