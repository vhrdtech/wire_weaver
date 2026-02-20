#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::Second;

/// defmt based logging.
/// Timestamps, if enabled, are encoded into the data stream.
#[ww_trait]
pub trait LogDefmt {
    /// Data either in raw format or in rzcobs encoding.
    /// When in raw - each stream event carries one log event.
    /// When in rzcobs - stream events boundaries do not have meaning, treat as continuous stream of bytes.
    stream!(data: [u8]);

    /// Which format is used by firmware.
    /// Raw can take more space, but requires less processing and framing (which stream provides).
    /// rzcobs - takes less space, but requires some processing.
    property!(ro format: DefmtFormat);

    /// Get current counters.
    fn counters() -> DefmtCounters;
    /// Reset counters to 0.
    fn reset_counters();
}

pub enum DefmtFormat {
    Raw,
    Rzcobs,
}

pub struct DefmtCounters {
    pub events_logged: u64,
    pub bytes_logged: u64,
    pub bytes_sent: u64,
}

/// Text based logging with configurable levels and optional timestamps.
#[ww_trait]
pub trait LogText {
    stream!(message: Message<'i>);
    fn enable_level(severity: Severity, enabled: bool);
    fn enabled_levels() -> [Severity];

    /// Get current counters.
    fn counters() -> TextCounters;
    /// Reset counters to 0.
    fn reset_counters();
}

pub struct Message<'i> {
    pub severity: Severity,
    pub timestamp: Option<Second>,
    pub contents: &'i str,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
pub enum Severity {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
    User(Nibble),
}

pub struct TextCounters {
    pub errors: u64,
    pub warnings: u64,
    pub info: u64,
    pub debug: u64,
    pub trace: u64,
}