use wire_weaver::prelude::*;
use ww_si::{Bytes, Count, Range, Second};

use crate::error::I2cErrorOwned;
use crate::{I2cAddress, I2cError, I2cReadKind, I2cReadKindOwned, Watch};

/// Most watches one `I2cWatch` device holds at once (it may hold fewer, see [I2cWatchCapabilities]).
pub const MAX_WATCHES: usize = 512;

/// Longest read of one watch, in bytes.
pub const MAX_WATCH_LEN: usize = 32;

/// Autonomous reads: the device repeats configured reads on its own and reports only what the host asked for
/// (every result, a change under a mask, or a match with a value), so the host doesn't poll over the link.
///
/// A device implementing it also implements `I2c` (it needs a master). Host transactions have priority: the device
/// runs a watch read only when no host transaction is waiting, so host calls are spliced in between autonomous
/// ones and see no more delay than one watch read. A watch to an address under a `delay_after` waits like any
/// transfer. Results go out on the `changes` stream.
#[ww_trait]
pub trait I2cWatch {
    /// Results the watches report, in the order they were read. A counted stream, unit: result; the head's time is
    /// when the read started, and the host learns exactly how many results the device dropped.
    #[counted]
    stream!(changes: I2cWatchEvent<'i>);

    /// How many watches, how fast, how long.
    fn watch_capabilities() -> I2cWatchCapabilities;

    /// Adds a watch and returns its slot, used by the other methods and in [I2cWatchEvent::slot].
    /// `InvalidArgument` for a period below the device's minimum or a mask or match rule not as long as the read,
    /// `BufferTooBig` for a read longer than this device watches, `WrongState` while not a master.
    #[errors(Common(InvalidArgument, WrongState), OutOfWatchSlots, BufferTooBig)]
    fn watch_add(watch: I2cWatchConfig<'i>) -> Result<WatchSlot, I2cError<'i>>;

    /// Replaces a watch's configuration; its previous result is forgotten, so the next read is reported. Errors as
    /// `I2cWatch::watch_add`.
    #[errors(Common(InvalidArgument, WrongState), InvalidWatchSlot, BufferTooBig)]
    fn watch_modify(slot: WatchSlot, watch: I2cWatchConfig<'i>) -> Result<(), I2cError<'i>>;

    /// Removes a watch.
    #[errors(InvalidWatchSlot)]
    fn watch_remove(slot: WatchSlot) -> Result<(), I2cError<'i>>;

    /// Slots in use.
    fn watch_list() -> MaxLen<RefVec<'i, WatchSlot>, MAX_WATCHES>;
}

/// A watch's slot, returned by `I2cWatch::watch_add` and used by `watch_modify`, `watch_remove`, `watch_list` and
/// [I2cWatchEvent::slot].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    transparent,
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone)
)]
pub struct WatchSlot(pub u32);

/// What `I2cWatch` can do.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Copy, Clone)
)]
pub struct I2cWatchCapabilities {
    /// Watches held at once, 1 to [MAX_WATCHES].
    pub max_watches: Count<Watch, u16, Range<1, { MAX_WATCHES as i64 }>>,
    /// Longest read of one watch, 1 to [MAX_WATCH_LEN] bytes.
    pub max_len: Bytes<u8, Range<1, { MAX_WATCH_LEN as i64 }>>,
    /// Shortest period of one watch.
    pub min_period: Second,
}

/// One autonomous read, see `I2cWatch::watch_add`.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cWatchConfig<'i> {
    pub addr: I2cAddress,
    /// Plain read, or a register read (write the register address, repeated start, read).
    pub kind: I2cReadKind<'i>,
    /// Bytes to read, 1 to [MAX_WATCH_LEN] (and at most [I2cWatchCapabilities::max_len]).
    pub len: Bytes<u8, Range<1, { MAX_WATCH_LEN as i64 }>>,
    /// How often to read.
    pub period: Second,
    /// When to send a result.
    pub report: I2cWatchReport<'i>,
}

/// When a watch sends its result.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone),
    ww_repr = unib32
)]
pub enum I2cWatchReport<'i> {
    /// Every read.
    Always,
    /// When `data & mask` differs from the previous read's (`mask` as long as the read; all ones to see any change).
    Changed {
        mask: MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>,
    },
    /// When `data[i] & bytes[i].mask == bytes[i].value` for every byte starts or stops being true (an alert bit set,
    /// a status reaching a state); one [I2cMatchByte] per byte read.
    Matches {
        bytes: MaxLen<RefVec<'i, I2cMatchByte>, MAX_WATCH_LEN>,
    },
}

/// One byte of an [I2cWatchReport::Matches] rule: the read byte matches when `byte & mask == value`. A `mask` of
/// `0x00` ignores the byte. Two bytes on the wire.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    sized
)]
pub struct I2cMatchByte {
    pub mask: u8,
    pub value: u8,
}

/// A result of a watch, on the `I2cWatch` `changes` stream; when the read started is the time in the event head. An error is reported once, when the watch starts to
/// fail, and the next successful read is reported whatever the rule.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Debug, Clone)
)]
pub struct I2cWatchEvent<'i> {
    pub slot: WatchSlot,
    pub result: Result<MaxLen<RefVec<'i, u8>, MAX_WATCH_LEN>, I2cError<'i>>,
}
