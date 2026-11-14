use wire_weaver::prelude::*;
use ww_si::Second;

use crate::{FilterIndex, TxMarker};

/// Largest data of a [Frame], in bytes (CAN FD).
pub const MAX_DATA_LEN: usize = 64;

/// An 11-bit (standard) or 29-bit (extended) CAN identifier.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing,
    ww_repr = u1,
    no_future_version
)]
pub enum CanId {
    Standard(U11),
    Extended(U29),
}

impl CanId {
    /// A standard ID, `None` above `0x7FF`.
    pub const fn standard(id: u16) -> Option<Self> {
        match U11::new(id) {
            Some(id) => Some(CanId::Standard(id)),
            None => None,
        }
    }

    /// An extended ID, `None` above `0x1FFF_FFFF`.
    pub const fn extended(id: u32) -> Option<Self> {
        match U29::new(id) {
            Some(id) => Some(CanId::Extended(id)),
            None => None,
        }
    }

    /// The ID as a number, 11 or 29 bits.
    pub const fn raw(&self) -> u32 {
        match self {
            CanId::Standard(id) => id.value() as u32,
            CanId::Extended(id) => id.value(),
        }
    }

    pub const fn is_extended(&self) -> bool {
        matches!(self, CanId::Extended(_))
    }
}

/// A CAN frame: classic data, remote or CAN FD.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Clone, Debug, PartialEq, Eq),
    final_structure
)]
pub struct Frame<'i> {
    pub id: CanId,
    pub kind: FrameKind,
    /// 0-8 bytes for a classic frame, empty for a remote frame, 0-8, 12, 16, 20, 24, 32, 48 or 64 bytes for an FD
    /// frame (a device pads other lengths up to the next one with zeroes).
    pub data: MaxLen<RefVec<'i, u8>, MAX_DATA_LEN>,
}

/// Kind of a [Frame].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u2,
    no_future_version
)]
pub enum FrameKind {
    /// Classic CAN data frame.
    Data,
    /// Classic CAN remote frame (RTR), asking for `len` bytes (its DLC, 0-8).
    Remote { len: U4 },
    /// CAN FD frame.
    Fd {
        /// Bit rate switch: the data phase runs at the data bit rate.
        brs: bool,
        /// Error state indicator: the sender was error passive.
        esi: bool,
    },
    /// Classic CAN data frame with 8 data bytes and a DLC of 9-15 on the wire (ISO 11898-1 allows them; SocketCAN
    /// `CAN_CTRLMODE_CC_LEN8_DLC`). For bus debuggers; nodes send [FrameKind::Data].
    DataLongDlc { dlc: U4 },
}

impl Frame<'_> {
    /// Data length of the frame (a remote frame's requested length).
    pub fn len(&self) -> usize {
        match self.kind {
            FrameKind::Remote { len } => len.value() as usize,
            _ => self.data.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_fd(&self) -> bool {
        matches!(self.kind, FrameKind::Fd { .. })
    }
}

/// A received frame, see `Can::rx`; when it was received is the time in the event head.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct RxFrame<'i> {
    pub frame: Frame<'i>,
    /// The filter element that accepted it (into [FilterConfig::filters]); `None` when it was accepted as
    /// non-matching, or the device has no filters.
    pub filter: Option<FilterIndex>,
}

/// A frame to send, see `Can::tx` and `Can::tx_buffer`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct TxFrame<'i> {
    pub frame: Frame<'i>,
    /// Comes back in the frame's [TxEvent], so a host matches events to the frames it sent; `None` when it doesn't
    /// need to (a device may then skip the event).
    pub marker: Option<TxMarker>,
}

/// A frame of a dedicated receive buffer, see `Can::rx_buffer`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct BufferedFrame<'i> {
    pub frame: Frame<'i>,
    /// When it was received, on the device's clock; `None` from a device without timestamps.
    pub time: Option<Second>,
}

/// What happened to a sent frame, see `Can::tx_events`; when is the time in the event head.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct TxEvent {
    /// [TxFrame::marker] of the frame.
    pub marker: Option<TxMarker>,
    pub id: CanId,
    pub outcome: TxOutcome,
    /// How long the frame waited from the device accepting it to its start of frame on the bus (queue, arbitration
    /// and retransmissions), measured on the device; `None` from a device that doesn't measure it.
    pub queued: Option<Second>,
}

/// Outcome of sending one frame.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum TxOutcome {
    /// Sent and acknowledged; the head's time is when it went on the bus.
    Sent,
    /// Dropped by `Can::tx_abort`, or by clearing its `Can::tx_buffer`.
    Aborted,
    /// One attempt failed with [Options::one_shot] (lost arbitration or an error).
    Failed,
    /// Not sendable as configured (an FD frame with FD off, a mode that doesn't send, bus-off with a full queue).
    Rejected,
}
