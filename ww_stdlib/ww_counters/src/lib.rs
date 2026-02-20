#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_log_bare_metal::Severity;

/// API for event counters running on a remote device.
/// Counters can be useful for counting errors, packets or any other events when it is impractical to log things, or in addition to logging.
///
/// Device can support different types of counters, for example:
/// * RAM based - reset to zero on device boot.
/// * Persistent - continue counting after reboot, or even after power cycle.
/// This trait can then be implemented for each kind.
#[ww_trait]
pub trait Counters {
    /// Number of RAM counters that are reset on reboot or power-on
    fn len() -> u32;
    /// Get current value of one RAM counter or None if idx is out of range.
    fn value(idx: u32) -> Option<CounterValue>;
    /// Get many counters values at once.
    fn values(filter: CountersFilter) -> RefVec<'i, CounterValue>;
    /// Get many counters values in a compressed form to save bandwidth. Optional.
    fn values_compressed(filter: CountersFilter) -> RefVec<'i, u8>;
    /// Stream of counter values changes. Optional, since it can be expensive to support this functionality.
    /// Stream sideband channel can also optionally be used to configure how often updates are sent, etc.
    stream!(values_changed: RefVec<'i, u32>);
    /// Set all counters to zero.
    fn reset();

    /// Get an array, describing each counter. Can be absent to conserve space.
    fn metadata() -> RefVec<'i, CounterMeta<'i>>;

    /// Counters implementation kind
    fn kind() -> CountersKind;
}

/// u32 or u64 counter value
#[derive_shrink_wrap]
#[ww_repr(u4)]
pub enum CounterValue {
    U32(u32),
    U64(u64),
}

/// Counters implementation kind, can be used as a debugging hint.
#[derive_shrink_wrap]
#[ww_repr(u4)]
pub enum CountersKind {
    /// RAM based counters, zeroed out on each boot.
    ResetOnBoot,
    /// RAM based counters, kept across reboots.
    ResetOnPowerCycle,
    /// Counters in a battery backed domain (BKPRAM, or similar).
    BatteryBackedUp,
    /// User kind.
    Other(Nibble),
}

/// Information about each counter.
#[derive_shrink_wrap]
pub struct CounterMeta<'i> {
    /// Can be empty to save space.
    /// Even if empty an original ELF can potentially be retrieved and used to get strings from it.
    pub name: &'i str,
    /// Error / Warning / etc
    pub severity: Severity,
}

/// Provides a way to select multiple counter indices.
#[derive_shrink_wrap]
#[ww_repr(u4)]
pub enum CountersFilter {
    All,
    /// Send only counter values with specified indices range.
    RangeInclusive {
        from: u32,
        to: u32,
    },
    /// Filter by severity, note indices will need to be mapped back to logical ones.
    /// For example by first calling metadata() to obtain necessary information.
    Severity(Severity),
}
