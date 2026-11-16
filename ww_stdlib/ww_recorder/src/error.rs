use ww_api::prelude::*;
use ww_error::CommonError;

/// Errors of the recorder calls.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    display,
    ww_repr = unib32
)]
pub enum RecorderError<'i> {
    /// Common error
    #[display("{0}")]
    #[common]
    Common(CommonError<'i>),
    /// No card in the slot.
    #[display("no card")]
    NoCard,
    /// The format token is expired, used, or from another card.
    #[display("format token expired or not valid for this card")]
    BadToken,
    /// The card check found a fake, broken or unreadable card.
    #[display("card failed the check: {reason:?}")]
    CardFailed { reason: MaxLen<&'i str, 64> },
    /// The recorder is not logging: no card, unbound, formatting or failed (see [Recorder::status]).
    #[display("not logging")]
    NotLogging,
    /// A recording is already running.
    #[display("a recording is running")]
    RecordingRunning,
    /// No recording is running.
    #[display("no recording is running")]
    NoRecording,
}

impl RecorderError<'_> {
    /// `Common(Unsupported)`.
    pub const fn unsupported() -> Self {
        RecorderError::Common(CommonError::Unsupported)
    }
}
