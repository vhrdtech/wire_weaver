//! Logs of bare metal firmware over WireWeaver: defmt frames decoded on the host with the firmware's ELF, or text
//! records for firmware without defmt.
//!
//! # Traits
//!
//! | Trait       | What it is for                                                        | Who implements it                     |
//! |-------------|-----------------------------------------------------------------------|---------------------------------------|
//! | `LogDefmt`  | The firmware's defmt log, whole frames, decoded on the host with the ELF. | Firmware logging with `defmt`      |
//! | `LogText`   | Text records with a level filter.                                     | Firmware without defmt (`log` crate, ESP-IDF) |
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyDevice {
//!     ww_impl!(log: ww_log::LogDefmt);
//! }
//! ```
//!
//! # Reliable streams
//!
//! Both logs are reliable streams (`docs/api/stream_reliability.md`): the device keeps what it logged in its store
//! until the host acknowledges it, also while no host is connected, so the logs from before the host connects
//! arrive when it does. When the store is full, the device's `FullPolicy` decides what goes (`DropOldest` keeps the
//! latest logs, which matter most after a fault), and the stream head's `lost` tells the host exactly how many log
//! messages it won't get: the unit of `LogDefmt::frames` is one defmt frame, the unit of `LogText::records` one
//! record. A store on SD or flash keeps the logs across reboots without any change to the API.
//!
//! A reliable stream needs an async server (`use_async = true`).
//!
//! # Timestamps
//!
//! defmt frames carry defmt's own `timestamp!` when the firmware defines one, decoded with the ELF; the head's `t`
//! stays off. Text records have their device time in the head's `t`.
//!
//! # The device side of `LogDefmt`
//!
//! `ww_defmt_logger` is a `#[defmt::global_logger]` that fills a lock-free ring, tees every frame to an RTT up
//! channel (`probe-rs` and `cnt run` keep showing the logs) and moves whole frames from the ring into the stream's
//! store from the event loop, counting frames that didn't fit as lost.
//!
//! # Errors
//!
//! Both traits return [LogError].
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Most bytes of defmt frames in one [DefmtFrames] item. A frame longer than this can't be sent and is counted as
/// lost. Raising it is compatible (the wire form doesn't change).
pub const MAX_FRAMES_LEN: usize = 512;

/// Longest build id in [DefmtInfo::build_id]: 20 bytes for the usual `--build-id=sha1`, up to 32 for others.
pub const MAX_BUILD_ID_LEN: usize = 32;

/// Longest [LogRecord::target] in bytes. Raising it is compatible.
pub const MAX_TARGET_LEN: usize = 48;

/// Longest [LogRecord::message] in bytes; a longer message is cut and marked [LogRecord::truncated]. Raising it is
/// compatible.
pub const MAX_MESSAGE_LEN: usize = 256;

/// defmt logs of the firmware, decoded on the host with its ELF (the format strings are interned there).
#[ww_trait]
pub trait LogDefmt {
    /// Encoding of the frames (defmt's `encoding-*` cargo feature) and the build id of the ELF with the format
    /// strings.
    const DEFMT: DefmtInfo<'i>;

    /// The log: whole defmt frames, kept in the device's store until the host acknowledges them, also those logged
    /// before the host connected. Unit: defmt frame, so the head's `lost` is the exact number of log messages the
    /// host won't get. `Busy` while another host has the log open, `Unsupported` when the device logs elsewhere only.
    #[errors(Common(Busy, Unsupported))]
    #[stream(DefmtFrames<'i>, reliable)]
    fn frames() -> Result<(), LogError<'i>>;
}

/// How to decode `LogDefmt::frames`, see `LogDefmt::DEFMT`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone)
)]
pub struct DefmtInfo<'i> {
    /// Encoding of the frames.
    pub encoding: DefmtEncoding,
    /// GNU build id of the firmware (`.note.gnu.build-id`): which ELF decodes the frames. Empty when the firmware
    /// didn't keep the note; a host then falls back to the newest ELF it finds and warns.
    pub build_id: MaxLen<RefVec<'i, u8>, MAX_BUILD_ID_LEN>,
}

/// defmt wire encoding, as chosen by defmt's `encoding-raw` / `encoding-rzcobs` cargo feature.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u4
)]
pub enum DefmtEncoding {
    /// Frames as defmt writes them, back to back.
    Raw,
    /// Each frame rzCOBS-encoded and ended by a `0x00` (defmt's default).
    Rzcobs,
}

/// One or more whole defmt frames, back to back, in [DefmtInfo::encoding]. Never half a frame: a decoder fed one
/// item after the other never has to resync, also with raw encoding and after a loss.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone)
)]
pub struct DefmtFrames<'i> {
    pub bytes: MaxLen<RefVec<'i, u8>, MAX_FRAMES_LEN>,
}

/// Text log for firmware without defmt (the `log` crate on embassy or std targets, ESP-IDF).
#[ww_trait]
pub trait LogText {
    /// The log: one record per message, kept in the device's store until the host acknowledges it, also those
    /// logged before the host connected. Unit: record. The head's `t` is the device time of the record. `Busy` while
    /// another host has the log open, `Unsupported` when the device logs elsewhere only.
    #[errors(Common(Busy, Unsupported))]
    #[stream(LogRecord<'i>, reliable)]
    fn records() -> Result<(), LogError<'i>>;

    /// Records less severe than this level are not produced (filters compiled into the firmware still apply).
    /// Writing it fails with `Unsupported` when the level is fixed at compile time.
    #[errors(Common(Unsupported))]
    property!(rw level: LevelFilter, LogError<'i>);
}

/// One text log message, see `LogText::records`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone)
)]
pub struct LogRecord<'i> {
    pub severity: Severity,
    /// Where it was logged, usually the module path (`usb_io_fw::power`). Cut to fit.
    pub target: MaxLen<&'i str, MAX_TARGET_LEN>,
    /// The formatted message, cut at the device's limit (at most [MAX_MESSAGE_LEN] bytes).
    pub message: MaxLen<&'i str, MAX_MESSAGE_LEN>,
    /// The message was cut.
    pub truncated: bool,
}

/// Severity of a [LogRecord], the five levels of `log`, `tracing` and `defmt`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone),
    no_future_version,
    ww_repr = u4
)]
pub enum Severity {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Which records `LogText` produces: `Info` means `Error`, `Warn` and `Info`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone),
    no_future_version,
    ww_repr = u4
)]
pub enum LevelFilter {
    /// Nothing is logged.
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LevelFilter {
    /// Whether a record of `severity` passes this filter.
    pub fn allows(self, severity: Severity) -> bool {
        let level = match severity {
            Severity::Error => LevelFilter::Error,
            Severity::Warn => LevelFilter::Warn,
            Severity::Info => LevelFilter::Info,
            Severity::Debug => LevelFilter::Debug,
            Severity::Trace => LevelFilter::Trace,
        };
        level <= self
    }
}

/// Errors of both traits in this crate. Each item says which variants it returns.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Clone),
    display,
    ww_repr = unib32
)]
pub enum LogError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_filter_allows() {
        assert!(!LevelFilter::Off.allows(Severity::Error));
        assert!(LevelFilter::Error.allows(Severity::Error));
        assert!(!LevelFilter::Error.allows(Severity::Warn));
        assert!(LevelFilter::Info.allows(Severity::Warn));
        assert!(!LevelFilter::Info.allows(Severity::Debug));
        assert!(LevelFilter::Trace.allows(Severity::Trace));
    }
}
