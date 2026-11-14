#![cfg_attr(not(feature = "std"), no_std)]
//! Control a recorder over WireWeaver (STD-34): a device that logs many tracks (streams of any type) to an SD card
//! or flash on its own, with sessions, recordings and markers.
//!
//! The first user is the logring engine (repo `logring`), but nothing here is specific to it: any recorder
//! that has a card, tracks and a ring can serve [Recorder]. A **track** is one stream declared in some trait of the
//! device's own API (a `stream!(..)` such as `can[1].rx`), identified by its numeric resource path ([TrackInfo]).
//! The recorder logs that stream's items, and a host reads them back through the same stream's own reliable
//! protocol (backfill), so the data never goes through [Recorder] itself: it only controls the card and the log.
//!
//! What the API covers:
//!
//! - [Recorder::status]: what the recorder is doing ([State]), the card, the running recording.
//! - [Recorder::card] and the two-step [Recorder::prepare_format] / [Recorder::format]: card facts, what a format
//!   would erase, and the check and erase itself only after the host passes back the offer's token.
//! - [Recorder::recording_start], [Recorder::recording_stop], [Recorder::recordings], [Recorder::marker]: labelling
//!   spans and moments of the log.
//! - [Recorder::settings] and the [Track] array: flush thresholds, full policy, USB view, per-track enable,
//!   durability and compression.
//! - [Recorder::counters] and [Track::counters]: bytes, drops, stalls, torn blocks, overwritten span.
//! - [Recorder::sync] and [Recorder::refresh_usb].
//!
//! Serve it with `ww api scaffold`, which prints the handler stubs for the `ww_codegen!` call you use.

use wire_weaver::prelude::*;
pub use ww_si::Second;
pub use ww_si::exact_units::niche::{Niched, Null};
use ww_si::prefix::One;
use ww_si::units::{Byte, Item, Per};
use ww_si::{Fixed, Mebibytes, Range, Secs};

pub mod card;
pub use card::{
    CardCheck, CardContent, CardFacts, CardKind, CardSerial, FormatAction, FormatOffer,
    FormatToken, MAX_NAME, Rating,
};
#[cfg(feature = "std")]
pub use card::{CardFactsOwned, FormatOfferOwned};
pub mod counters;
pub use counters::RecorderCounters;
pub mod error;
pub use error::RecorderError;
#[cfg(feature = "std")]
pub use error::RecorderErrorOwned;
pub mod recording;
pub use recording::{MarkerKind, RecordingId, RecordingInfo, SessionId, StopReason, Trigger};
pub mod settings;
pub use settings::{Compression, Durability, FullPolicy, RecordingMode, Settings, UsbView};
pub mod status;
pub use status::{State, Status};
#[cfg(feature = "std")]
pub use status::{StateOwned, StatusOwned};
pub mod track;
#[cfg(feature = "std")]
pub use track::TrackInfoOwned;
pub use track::{ItemType, MAX_PATH, Track, TrackCounters, TrackInfo, TrackKey};

/// Most recordings one [Recorder::recordings] call returns.
pub const MAX_RECORDINGS: usize = 8;
/// Longest label of a [Recorder::marker], in bytes.
pub const MAX_LABEL: usize = 64;

/// A size in bytes, `0` meaning unknown: [Niched] of [Null]`<0>`, the same 4 bytes as a plain `u32`. See
/// [CardFacts::au] and [CardFacts::recording_unit].
type UnknownableBytes = Niched<Fixed<Byte, One, u32, Range<1, 4_294_967_295>>, Null<0>, u32>;

/// Items per second, see [TrackInfo::rate].
type ItemRate = Per<Item, ww_si::units::Second>;

/// A track's nominal rate, `0` meaning unknown, same shape as [UnknownableBytes]. See [TrackInfo::rate].
type UnknownableRate = Niched<Fixed<ItemRate, One, u32, Range<1, 4_294_967_295>>, Null<0>, u32>;

/// Control of a recorder: a logger that writes tracks to a card or flash.
///
/// # Errors
///
/// [RecorderError]: the `Common` variants and the recorder's own, listed on each call.
#[ww_trait]
pub trait Recorder {
    /// Tracks of this session, indexed by track number: 0 is the system track (markers, recordings, drops, card
    /// events), the data tracks follow from 1. The indices valid right now are the session's tracks; they change
    /// only when the device boots.
    ww_impl!(track[]: Track);

    /// What the recorder is doing now.
    fn status() -> Status<'i>;

    /// Facts about the card in the slot, `None` without a card. Reads what the card reports (SD Status, CID); the
    /// speed rating is the result of the last card check, made by [Recorder::format].
    fn card() -> Option<CardFacts<'i>>;

    /// First step of a format: reads the card (facts, what is on it, limits) and answers what a format would do,
    /// with a one-use token for the second step. Writes nothing; logging goes on until [Recorder::format].
    ///
    /// `action` chooses a full format (erases everything, new rw region size) or re-creating the ring file after
    /// the card refused the old one (keeps the rw files).
    ///
    /// `Common(Busy)` while a format is running, `NoCard`.
    #[optional(formatting)]
    #[errors(Common(Busy, Unsupported), NoCard)]
    fn prepare_format(action: FormatAction) -> Result<FormatOffer<'i>, RecorderError<'i>>;

    /// Second step: carries out the offer. `rw_mb` is the size of the read-write region for a full format (`None`:
    /// the offer's default; ignored when re-creating the ring file). The token is valid for one call, only for the
    /// card the offer was made for, and expires after [FormatOffer::valid_for]. Stops logging, runs the
    /// card check (aliasing test, sequential write speed, worst stall, best flush size: it overwrites the card, which
    /// is why it waits for the token), then erases and writes the new layout. Takes as long as the card needs:
    /// progress is [State::Formatting] in [Recorder::status], the check's result [CardFacts::check].
    ///
    /// `BadToken` for an expired, used or other card's token, `Common(InvalidArgument)` for an `rw_mb` outside the
    /// offer's limits, `CardFailed` when the check finds a fake or broken card (nothing is formatted then).
    #[optional(formatting)]
    #[errors(Common(Busy, InvalidArgument), NoCard, BadToken, CardFailed)]
    fn format(token: FormatToken, rw_mb: Option<Mebibytes<u32>>) -> Result<(), RecorderError<'i>>;

    /// Starts a recording: a labelled span of the log. Everything is logged all the time in the default
    /// [RecordingMode::Continuous], so `pre_trigger` can set the start back by that long; in
    /// `Triggered` mode tracks are written only inside recordings and the pre-trigger needs a RAM ring per track
    /// (`Unsupported` if the device has none). Returns the recording id (counts up for the life of the card).
    ///
    /// `NotLogging` without a mounted ring, `RecordingRunning` while one is open.
    #[optional(recording)]
    #[errors(Common(Unsupported, InvalidArgument), NotLogging, RecordingRunning)]
    fn recording_start(
        trigger: Trigger,
        pre_trigger: Secs<u16>,
    ) -> Result<RecordingId, RecorderError<'i>>;

    /// Stops the running recording and returns its id. `NoRecording` when none runs.
    #[optional(recording)]
    #[errors(NotLogging, NoRecording)]
    fn recording_stop() -> Result<RecordingId, RecorderError<'i>>;

    /// Recordings on the card, oldest first, up to [MAX_RECORDINGS] with id above `after` (0 for the first page).
    /// Recordings whose data the ring has already overwritten are not listed.
    #[optional(recording)]
    #[errors(NotLogging)]
    fn recordings(
        after: RecordingId,
    ) -> Result<MaxLen<RefVec<'i, RecordingInfo>, MAX_RECORDINGS>, RecorderError<'i>>;

    /// Drops a marker into the system track: a moment to find again (a button press, a note, a trigger). Written
    /// at once, not with the next flush.
    #[optional(markers)]
    #[errors(Common(InvalidArgument), NotLogging)]
    fn marker(
        kind: MarkerKind,
        label: MaxLen<&'i str, MAX_LABEL>,
        value: Option<i64>,
    ) -> Result<(), RecorderError<'i>>;

    /// Writes what is in RAM to the card now (all tracks), instead of waiting for [Settings::flush] or
    /// [Settings::flush_interval]. Answers when it is on the card. Before a planned power-off, on a brown-out warning.
    #[errors(NotLogging)]
    fn sync() -> Result<(), RecorderError<'i>>;

    /// Settings: flush thresholds, full policy, recording mode, USB view. Each field says when it takes effect.
    #[errors(Common(InvalidArgument, Unsupported))]
    property!(rw settings: Settings, RecorderError<'i>);

    /// Totals since the card was formatted and since this boot.
    #[optional(counting)]
    fn counters() -> RecorderCounters;

    /// Tells the USB mass storage view to show the log as it is now. The view is a snapshot taken when the host
    /// mounts it, so new data shows only after a re-mount; this raises a media change instead. Optional: a device
    /// without a USB view leaves it out.
    #[optional(usb)]
    #[errors(Common(Unsupported, Busy))]
    fn refresh_usb() -> Result<(), RecorderError<'i>>;
}
