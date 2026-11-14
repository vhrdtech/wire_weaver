use wire_weaver::prelude::*;
use ww_si::prefix::Centi;
use ww_si::units::Ratio;
use ww_si::{Bytes, Fixed, Range, Second};

use crate::{RecordingId, SessionId};

/// A percentage within 0..=100, see [State::Formatting].
type BoundedPercent = Fixed<Ratio, Centi, u8, Range<0, 100>>;

/// What the recorder is doing, see [Recorder::status].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Status<'i> {
    pub state: State<'i>,
    /// Session of this boot, 0 without a mounted ring.
    pub session: SessionId,
    /// The running recording, if any.
    pub recording: Option<RecordingId>,
    /// Device time of the newest record, `None` before the first.
    pub device_time: Option<Second>,
    /// The ring holding data now, and its capacity. Equal once the ring went round.
    pub ring_used: Bytes<u64>,
    pub ring_capacity: Bytes<u64>,
}

/// The recorder's state, see [Status::state].
///
/// `Unsized` (default): a later version may add a state without breaking a host built against this one.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq),
    ww_repr = unib32
)]
pub enum State<'i> {
    /// No card in the slot.
    NoCard,
    /// A card without a log (new, or another system's): offer a format.
    NeedsFormat,
    /// The card has a ring file but it is not where the device put it (moved, deleted, replaced by a PC, other
    /// card): logging stays off until the file is re-created or the card formatted. Never overwrites anything.
    Unbound { reason: MaxLen<&'i str, 64> },
    /// A check or format is running.
    Formatting { percent: BoundedPercent },
    /// Logging.
    Logging,
    /// The ring is full and the full policy is `DropNewest`: new items are refused and counted.
    Full,
    /// The card failed (write error, removed while logging): logging is off, `reason` says why.
    Failed { reason: MaxLen<&'i str, 64> },
}
