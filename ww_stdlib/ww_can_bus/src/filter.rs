use wire_weaver::prelude::*;

use crate::{BufferIndex, CanId, FifoIndex};

/// Most filter elements of one `Can::set_filters`.
pub const MAX_FILTERS: usize = 256;

/// Acceptance filters, see `Can::set_filters`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct FilterConfig<'i> {
    /// Filter elements, first match wins.
    pub filters: MaxLen<RefVec<'i, Filter>, MAX_FILTERS>,
    /// What happens to a standard frame no filter matches.
    pub non_matching_standard: NonMatching,
    /// What happens to an extended frame no filter matches.
    pub non_matching_extended: NonMatching,
    /// Drop every remote frame.
    pub reject_remote: bool,
}

/// One filter element: which IDs it matches and what it does with them.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Filter {
    pub matching: FilterMatch,
    pub action: FilterAction,
}

/// IDs a [Filter] matches. Standard and extended IDs never match each other.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FilterMatch {
    /// One ID.
    Exact(CanId),
    /// IDs equal to `id` in every bit set in `mask`.
    Mask { id: CanId, mask: U29 },
    /// IDs from `from` to `to`, both included, both of the same kind.
    Range { from: CanId, to: CanId },
}

/// What a [Filter] does with a frame it matches.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FilterAction {
    /// Into receive FIFO `fifo` (0 when the controller has one), and from there on `rx`; `high_priority` makes the
    /// device pass it on ahead of other frames (M_CAN high priority message).
    Fifo {
        fifo: FifoIndex,
        high_priority: bool,
    },
    /// Into dedicated receive buffer `index`, read with `Can::rx_buffer`; not sent on `rx`.
    Buffer(BufferIndex),
    /// Dropped.
    Reject,
}

/// What happens to a frame no [Filter] matches.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum NonMatching {
    /// Into receive FIFO `fifo`.
    Accept {
        fifo: FifoIndex,
    },
    Reject,
}
