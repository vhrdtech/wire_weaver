//! What a reliable stream keeps until the host acknowledges it: the [StreamStore] trait and the stores shipped
//! with this crate.
//!
//! A store keeps whole events, as they go on the wire: the [StreamHead] (units and timestamp included) and the item
//! bytes. A resent event is bit-identical to the first send, so timestamps are always
//! the device's time when the data was made.
//!
//! Positions are the events' `pos`, `u64` units since the session started. A store keeps events in position order
//! and never more than one session.

use crate::StreamHead;

pub(crate) mod ram_ring;
pub use ram_ring::{RECORD_OVERHEAD, RamRing, RamRingError};

#[cfg(any(test, feature = "persistent-ram"))]
mod persistent_ram;
#[cfg(any(test, feature = "persistent-ram"))]
pub use persistent_ram::{
    PERSISTENT_RECORD_OVERHEAD, PersistentRamRing, RamArea, RamRecovery, SLOT_LEN,
};

#[cfg(any(test, feature = "log-store"))]
pub mod log;
#[cfg(any(test, feature = "log-store"))]
pub use log::{LogConfig, LogError, LogMedium, LogStore, Recovery};

/// Storage of the events of one reliable stream, see the [module docs](self).
///
/// Methods are `async` because a store may sit on an SD card or flash; RAM stores never wait.
#[allow(async_fn_in_trait)]
pub trait StreamStore {
    /// Error of the underlying storage (a block device, flash).
    type Error;

    /// Append an event: its head and item bytes, as they go on the wire.
    ///
    /// `head.pos` is above every position stored before. A store that drops the oldest events when full makes
    /// room by forgetting them (`state().oldest` moves on); one that drops the newest returns [PushError::Full]
    /// and stores nothing, the caller counts the units as lost.
    async fn push(&mut self, head: &StreamHead, item: &[u8]) -> Result<(), PushError<Self::Error>>;

    /// Copy the event at `pos`, or the first one after it, into `buf`. `None` when there is none.
    ///
    /// `buf` must hold the biggest item pushed, a smaller one is an error of the store.
    async fn read<'b>(
        &mut self,
        pos: u64,
        buf: &'b mut [u8],
    ) -> Result<Option<StoredEvent<'b>>, Self::Error>;

    /// Events before `pos` are acknowledged and may go. Events at or after `pos` are kept.
    async fn release(&mut self, pos: u64) -> Result<(), Self::Error>;

    /// Session, oldest and newest kept positions, next position, bytes used and capacity, persistent or not.
    fn state(&self) -> StoreState;

    /// A new session: drop everything, positions from 0.
    async fn reset(&mut self, session: u32) -> Result<(), Self::Error>;
}

/// An event read back from a [StreamStore].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoredEvent<'b> {
    /// Head as pushed.
    pub head: StreamHead,
    /// Item bytes as pushed.
    pub item: &'b [u8],
}

/// What a [StreamStore] holds, see [StreamStore::state].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StoreState {
    /// Session the kept events belong to. Sessions with bit 31 set are reserved for stores that start a stream by
    /// themselves (a logring boot that didn't recover positions); hosts and firmware choose sessions with it clear.
    pub session: u32,
    /// Position of the oldest kept event, `None` when empty.
    pub oldest: Option<u64>,
    /// Position of the newest kept event, `None` when empty.
    pub newest: Option<u64>,
    /// Position right after the newest event's units (0 in a new session): where the stream continues, unless units
    /// were lost after it. A persistent store gives the sender its position back after a reboot.
    pub next: u64,
    /// Bytes in use, including the store's own overhead.
    pub used_bytes: u64,
    /// Bytes the store can hold.
    pub capacity_bytes: u64,
    /// Survives a reboot (a log on SD or flash) or a soft reset (RAM kept across it), not just a reconnect.
    pub persistent: bool,
    /// Units of the events the store forgot to make room (drop oldest), since the session started. Not those
    /// released after an acknowledgement, nor those [PushError::Full] refused.
    pub dropped_units: u64,
}

/// Why [StreamStore::push] stored nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError<E> {
    /// No room (drop newest): the caller counts the event's units as lost.
    Full,
    /// The event is bigger than the whole store: it can never be stored, counted as lost like [PushError::Full].
    TooBig,
    /// The underlying storage failed.
    Store(E),
}

/// What a store does when an event doesn't fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FullPolicy {
    /// Keep what is stored, drop the new event ([PushError::Full]): what is kept stays contiguous (default).
    #[default]
    DropNewest,
    /// Forget the oldest events until the new one fits: a ring of the most recent data.
    DropOldest,
}
