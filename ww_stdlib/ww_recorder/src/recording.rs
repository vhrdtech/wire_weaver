use wire_weaver::prelude::*;
use ww_si::{Bytes, Second};

/// Session of a boot (counts up for the life of the card), see [Status::session] and [RecordingInfo::session].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    transparent
)]
pub struct SessionId(pub u32);

/// Id of a recording, counting up for the life of the card, see [RecordingInfo::id].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    transparent
)]
pub struct RecordingId(pub u32);

/// What started a recording, see [Recorder::recording_start] and [RecordingInfo].
///
/// `Unsized` (default): more trigger kinds (conditions on a track) are planned.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Trigger {
    /// A call of [Recorder::recording_start].
    Api,
    /// The device's button.
    Button,
}

/// Why a recording ended, see [RecordingInfo::stopped].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum StopReason {
    /// [Recorder::recording_stop].
    Api,
    /// The button.
    Button,
    /// The configured duration passed.
    Duration,
    /// The device rebooted or lost power and did not continue it.
    Reboot,
}

/// One recording of [Recorder::recordings].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct RecordingInfo {
    pub id: RecordingId,
    /// Session it started in.
    pub session: SessionId,
    pub trigger: Trigger,
    /// Device time of the start (shifted back by the pre-trigger), `None` if the clock was not running.
    pub start: Option<Second>,
    /// `None` while it runs.
    pub stopped: Option<StopReason>,
    /// Length in device time, `None` while it runs or unknown.
    pub duration: Option<Second>,
    /// The ring the span covers, all tracks.
    pub bytes: Bytes<u64>,
}

/// What a marker is for, see [Recorder::marker].
///
/// On the card a kind is one byte: [MarkerKind::code].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum MarkerKind {
    /// Set by a call of [Recorder::marker].
    Api,
    /// The device's button.
    Button,
    /// A trigger condition.
    Trigger,
    /// Anything else the application defines, 3 and up on the card.
    User { code: u8 },
}

impl MarkerKind {
    /// The kind byte stored in the log.
    pub const fn code(&self) -> u8 {
        match self {
            MarkerKind::Api => 0,
            MarkerKind::Button => 1,
            MarkerKind::Trigger => 2,
            MarkerKind::User { code } => {
                if *code < 3 {
                    3
                } else {
                    *code
                }
            }
            // a kind from a newer API version is stored as a user marker
            MarkerKind::Unknown(_) => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_codes() {
        assert_eq!(MarkerKind::Api.code(), 0);
        assert_eq!(MarkerKind::Button.code(), 1);
        assert_eq!(MarkerKind::User { code: 1 }.code(), 3);
        assert_eq!(MarkerKind::User { code: 200 }.code(), 200);
    }
}
