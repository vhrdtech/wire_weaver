#![cfg_attr(not(feature = "std"), no_std)]
//! Control a recorder over WireWeaver (STD-34): a device that logs many tracks (streams of any type) to an SD card
//! or flash on its own, with sessions, recordings and markers.
//!
//! The first user is `ww_logring` (P2628, repo `logring`), but nothing here is specific to it: any recorder that
//! has a card, tracks and a ring can serve [Recorder]. The data itself is not carried by this API. Every track is a
//! stream of some `#[ww_trait]` in the device's API, so a host reads logged data back (backfill) with that
//! stream's own reliable protocol, and the recorder is controlled with the calls here.
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
use ww_error::CommonError;
pub use ww_si::Second;

/// Most recordings one [Recorder::recordings] call returns.
pub const MAX_RECORDINGS: usize = 8;
/// Longest label of a [Recorder::marker], in bytes.
pub const MAX_LABEL: usize = 64;
/// Longest name in the status and card facts (card product name, track path), in bytes.
pub const MAX_NAME: usize = 48;

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
    #[errors(Common(Busy, Unsupported), NoCard)]
    fn prepare_format(action: FormatAction) -> Result<FormatOffer<'i>, RecorderError<'i>>;

    /// Second step: carries out the offer. `rw_mb` is the size of the read-write region for a full format (`None`:
    /// the offer's default; ignored when re-creating the ring file). The token is valid for one call, only for the
    /// card the offer was made for, and expires after [FormatOffer::valid_for_s] seconds. Stops logging, runs the
    /// card check (aliasing test, sequential write speed, worst stall, best flush size: it overwrites the card, which
    /// is why it waits for the token), then erases and writes the new layout. Takes as long as the card needs:
    /// progress is [State::Formatting] in [Recorder::status], the check's result [CardFacts::check].
    ///
    /// `BadToken` for an expired, used or other card's token, `Common(InvalidArgument)` for an `rw_mb` outside the
    /// offer's limits, `CardFailed` when the check finds a fake or broken card (nothing is formatted then).
    #[errors(Common(Busy, InvalidArgument), NoCard, BadToken, CardFailed)]
    fn format(token: u32, rw_mb: Option<u32>) -> Result<(), RecorderError<'i>>;

    /// Starts a recording: a labelled span of the log. Everything is logged all the time in the default
    /// [RecordingMode::Continuous], so `pre_trigger_s` can set the start back by that many seconds; in
    /// `Triggered` mode tracks are written only inside recordings and the pre-trigger needs a RAM ring per track
    /// (`Unsupported` if the device has none). Returns the recording id (counts up for the life of the card).
    ///
    /// `NotLogging` without a mounted ring, `RecordingRunning` while one is open.
    #[errors(Common(Unsupported, InvalidArgument), NotLogging, RecordingRunning)]
    fn recording_start(trigger: Trigger, pre_trigger_s: u16) -> Result<u32, RecorderError<'i>>;

    /// Stops the running recording and returns its id. `NoRecording` when none runs.
    #[errors(NotLogging, NoRecording)]
    fn recording_stop() -> Result<u32, RecorderError<'i>>;

    /// Recordings on the card, oldest first, up to [MAX_RECORDINGS] with id above `after` (0 for the first page).
    /// Recordings whose data the ring has already overwritten are not listed.
    #[errors(NotLogging)]
    fn recordings(
        after: u32,
    ) -> Result<MaxLen<RefVec<'i, RecordingInfo>, MAX_RECORDINGS>, RecorderError<'i>>;

    /// Drops a marker into the system track: a moment to find again (a button press, a note, a trigger). Written
    /// at once, not with the next flush.
    #[errors(Common(InvalidArgument), NotLogging)]
    fn marker(
        kind: MarkerKind,
        label: MaxLen<&'i str, MAX_LABEL>,
        value: Option<i64>,
    ) -> Result<(), RecorderError<'i>>;

    /// Writes what is in RAM to the card now (all tracks), instead of waiting for `flush_bytes` or
    /// `flush_interval_ms`. Answers when it is on the card. Before a planned power-off, on a brown-out warning.
    #[errors(NotLogging)]
    fn sync() -> Result<(), RecorderError<'i>>;

    /// Settings: flush thresholds, full policy, recording mode, USB view. Each field says when it takes effect.
    #[errors(Common(InvalidArgument, Unsupported))]
    property!(rw settings: Settings, RecorderError<'i>);

    /// Totals since the card was formatted and since this boot.
    fn counters() -> RecorderCounters;

    /// Tells the USB mass storage view to show the log as it is now. The view is a snapshot taken when the host
    /// mounts it, so new data shows only after a re-mount; this raises a media change instead. Optional: a device
    /// without a USB view leaves it out.
    #[optional]
    #[errors(Common(Unsupported, Busy))]
    fn refresh_usb() -> Result<(), RecorderError<'i>>;
}

/// One track of the log, `Recorder::track[n]`.
#[ww_trait]
pub trait Track {
    /// Where the track comes from and what it carries.
    fn info() -> TrackInfo<'i>;

    /// Whether the track is written. A disabled track keeps its place in the table and its position; its items are
    /// counted as dropped. Takes effect at once; the system track (0) can't be disabled (`Unsupported`).
    #[errors(Common(Unsupported))]
    property!(rw enabled: bool, RecorderError<'i>);

    /// When the track is flushed to the card, see [Durability]. Takes effect at once.
    #[errors(Common(Unsupported))]
    property!(rw durability: Durability, RecorderError<'i>);

    /// Compression of the track's batches, see [Compression]. Optional: a device without a codec leaves it out,
    /// and tracks are stored plain.
    #[optional]
    #[errors(Common(Unsupported))]
    property!(rw compression: Compression, RecorderError<'i>);

    /// What the track has stored and lost.
    fn counters() -> TrackCounters;
}

/// What the recorder is doing, see [Recorder::status].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct Status<'i> {
    pub state: State<'i>,
    /// Session of this boot (counts up for the life of the card), 0 without a mounted ring.
    pub session: u32,
    /// The running recording, if any.
    pub recording: Option<u32>,
    /// Device time of the newest record, `None` before the first.
    pub device_time: Option<Second>,
    /// Bytes of the ring holding data now, and its capacity. Equal once the ring went round.
    pub ring_used_bytes: u64,
    pub ring_capacity_bytes: u64,
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
    /// A check or format is running. `percent` 0..=100.
    Formatting { percent: u8 },
    /// Logging.
    Logging,
    /// The ring is full and the full policy is `DropNewest`: new items are refused and counted.
    Full,
    /// The card failed (write error, removed while logging): logging is off, `reason` says why.
    Failed { reason: MaxLen<&'i str, 64> },
}

/// The card, see [Recorder::card].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct CardFacts<'i> {
    pub kind: CardKind,
    /// Capacity the card reports, in bytes.
    pub capacity_bytes: u64,
    /// Product name (CID, e.g. `SD64G`), empty if the medium has none.
    pub product: MaxLen<&'i str, MAX_NAME>,
    /// Manufacturer id (CID).
    pub manufacturer: u8,
    /// Serial number (CID).
    pub serial: u32,
    /// Allocation unit in bytes (SD Status `AU_SIZE`, the larger UHS/video one where given), 0 if the card reports
    /// none: the log then uses 4 MB.
    pub au_bytes: u32,
    /// Recording unit of the card's speed class in bytes (16 KB and up), 0 unknown.
    pub recording_unit_bytes: u32,
    /// Speed class (`SPEED_CLASS`: 2, 4, 6, 10), 0 unknown.
    pub speed_class: u8,
    /// UHS speed grade (1, 3), 0 unknown.
    pub uhs_grade: u8,
    /// Video speed class (6, 10, 30, 60, 90), 0 unknown.
    pub video_class: u8,
    /// What is on the card.
    pub content: CardContent,
    /// Result of the last card check, `None` before one.
    pub check: Option<CardCheck>,
}

/// Kind of medium, see [CardFacts::kind].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum CardKind {
    Sdsc,
    Sdhc,
    Sdxc,
    /// Soldered or socketed eMMC.
    Emmc,
    /// SPI or QSPI NOR flash.
    NorFlash,
}

/// What is on a card, see [CardFacts::content].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum CardContent {
    /// Nothing recognised (blank, or a filesystem the device doesn't read).
    Unrecognised,
    /// A volume with files and no ring.
    OtherFiles,
    /// A ring this device mounts.
    Ring,
    /// A ring file that is not where the device put it, see [State::Unbound].
    UnboundRing,
}

/// Result of a card check, see [CardFacts::check]. Rates the card against [Settings] and the data rate configured.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct CardCheck {
    /// Capacity really there: a fake card wraps around when written past its true size.
    pub aliasing_ok: bool,
    /// Sustained sequential write speed in KiB/s.
    pub write_kib_s: u32,
    /// Longest single write seen in milliseconds.
    pub worst_stall_ms: u32,
    /// Flush size that wrote fastest on this card in bytes (0: not measured): the device's default for
    /// [Settings::flush_bytes].
    pub best_flush_bytes: u32,
    pub rating: Rating,
}

/// How the card does against the configured data rate, see [CardCheck::rating].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Rating {
    /// Keeps up with the configured rate with the RAM buffer's margin for its stalls.
    Good,
    /// Keeps up on average, but its worst stall is longer than the buffer holds: drops are possible.
    Marginal,
    /// Slower than the configured rate: drops are certain.
    TooSlow,
    /// Not checked.
    Unchecked,
}

/// What a format starts from, see [Recorder::prepare_format].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FormatAction {
    /// Erase the card: new filesystem, new ring, new read-write region.
    Format,
    /// Create the ring file again after [State::Unbound], keeping the files of the read-write region.
    RecreateRing,
}

/// What a format would do, the answer of [Recorder::prepare_format].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct FormatOffer<'i> {
    /// Pass to [Recorder::format].
    pub token: u32,
    /// Seconds the token stays valid.
    pub valid_for_s: u16,
    pub action: FormatAction,
    /// The card, with the result of its last check if it had one.
    pub card: CardFacts<'i>,
    /// Bytes the second step erases: the whole card for [FormatAction::Format].
    pub erases_bytes: u64,
    /// Whether the card holds a ring with data, and files in its read-write region. A UI warns on these.
    pub has_ring_data: bool,
    pub has_rw_files: bool,
    /// Read-write region size: the default and the limits, in MB.
    pub rw_mb_default: u32,
    pub rw_mb_min: u32,
    pub rw_mb_max: u32,
    /// Bytes the ring gets with the default region.
    pub ring_bytes: u64,
}

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
    pub id: u32,
    /// Session it started in.
    pub session: u32,
    pub trigger: Trigger,
    /// Device time of the start (shifted back by the pre-trigger), `None` if the clock was not running.
    pub start: Option<Second>,
    /// `None` while it runs.
    pub stopped: Option<StopReason>,
    /// Length in device time, `None` while it runs or unknown.
    pub duration: Option<Second>,
    /// Bytes of the ring the span covers, all tracks.
    pub bytes: u64,
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

/// Settings of the recorder, see [Recorder::settings].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Settings {
    /// Bytes a flush grows to before it is written, a multiple of 512 and the card's recording unit, at most the
    /// ring's segment payload. Default 64 KB, or the card check's [CardCheck::best_flush_bytes]. Larger writes
    /// are faster and lose more in a power cut. Takes effect at once.
    pub flush_bytes: u32,
    /// Milliseconds after which a flush with data is written even if small (default 1000): the most a power cut
    /// takes when data comes slowly. Takes effect at once.
    pub flush_interval_ms: u32,
    /// What to do when the ring is full. Takes effect from the next boot.
    pub full: FullPolicy,
    /// Whether tracks are written all the time or only in recordings. Takes effect from the next boot.
    pub mode: RecordingMode,
    /// How the log is split in the USB view. Takes effect at the next [Recorder::refresh_usb] or mount.
    pub usb_view: UsbView,
}

/// Behaviour of a full ring, see [Settings::full].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FullPolicy {
    /// A logger: overwrite the oldest data.
    DropOldest,
    /// Stop writing and count what is refused ([State::Full]).
    DropNewest,
}

/// Which items are written, see [Settings::mode].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum RecordingMode {
    /// Everything is logged all the time; a recording labels a span, so the pre-trigger time is free.
    Continuous,
    /// Tracks are written only inside recordings.
    Triggered,
}

/// How the USB view splits the log, see [Settings::usb_view]. The recordings folder is always there.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct UsbView {
    /// A folder per hour of device or real time.
    pub by_time: bool,
    /// A folder per track.
    pub by_track: bool,
    /// Fixed-width CSV files next to the native ones, for tracks that have one.
    pub csv: bool,
}

/// When a track is flushed, see [Track::durability].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Durability {
    /// With the next flush (`flush_bytes` or `flush_interval_ms`).
    Normal,
    /// Written to the card right after each item: for events and errors. Costs a write per item.
    Immediate,
}

/// Compression of a track's batches, see [Track::compression].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Compression {
    None,
    Lz4,
    Heatshrink,
}

/// A track's identity, see [Track::info].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct TrackInfo<'i> {
    /// The stream's resource path in the device API, with indices: `can[1].rx`. The system track is `system`.
    pub path: MaxLen<&'i str, MAX_NAME>,
    /// The item type: crate, version and path, as the PC tool finds it in the API bundle.
    pub item_type: MaxLen<&'i str, MAX_NAME>,
    /// What a position counts: `bytes`, `frames`, `samples`. May be empty.
    pub units: MaxLen<&'i str, 16>,
    /// Stable 64-bit key: the same track across sessions and firmware versions that keep its path.
    pub key: u64,
    /// Nominal items per second, 0 unknown.
    pub rate: u32,
}

/// What a track has stored, see [Track::counters]. Since this boot unless it says otherwise.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TrackCounters {
    /// Records and item bytes stored.
    pub records: UVlq64,
    pub bytes: UVlq64,
    /// Where the track's stream continues (counts across boots, the stream position of the next item).
    pub next: UVlq64,
    /// Up to where the track is on the card.
    pub durable: UVlq64,
    /// Up to where the host released it.
    pub released: UVlq64,
    /// Units refused or lost on the way: producer overflow, disabled track, full ring.
    pub dropped: UVlq64,
}

/// Totals, see [Recorder::counters]. Numbers are variable length: an idle recorder's answer is a few bytes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct RecorderCounters {
    /// Data and meta bytes written since the format.
    pub bytes_written: UVlq64,
    /// Data segments the ring went round and overwrote since the format; with `ring_segments` the overwritten
    /// span.
    pub segments_reused: UVlq64,
    pub ring_segments: UVlq64,
    /// Blocks and records a mount found torn by a power cut or damaged by the card, this boot and since the
    /// format.
    pub torn_blocks: UVlq32,
    pub torn_records: UVlq32,
    /// Units lost in all tracks, this boot.
    pub dropped: UVlq64,
    /// Writes slower than the stall threshold, and the longest, this boot.
    pub card_stalls: UVlq32,
    pub worst_stall_ms: UVlq32,
    /// Write errors of the card, this boot.
    pub card_errors: UVlq32,
    /// Sessions since the format (this one included).
    pub sessions: UVlq32,
    /// Device time of the oldest data still in the ring, `None` if empty or unknown.
    pub oldest: Option<Second>,
}

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

    #[test]
    fn settings_round_trip() {
        let s = Settings {
            flush_bytes: 64 << 10,
            flush_interval_ms: 1000,
            full: FullPolicy::DropOldest,
            mode: RecordingMode::Continuous,
            usb_view: UsbView {
                by_time: true,
                by_track: false,
                csv: true,
            },
        };
        let mut buf = [0u8; 64];
        let bytes = s.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(Settings::from_ww_bytes(bytes).unwrap(), s);
    }

    #[test]
    fn counters_are_small_when_idle() {
        let mut buf = [0u8; 128];
        let z = UVlq64(0);
        let idle = TrackCounters {
            records: z,
            bytes: z,
            next: z,
            durable: z,
            released: z,
            dropped: z,
        };
        let bytes = idle.to_ww_bytes(&mut buf).unwrap();
        assert!(bytes.len() <= 8, "{} bytes", bytes.len());
    }
}
