//! Device side of the I2C and SPI slave modes of `ww_i2c` and `ww_spi` (STD-35): what a firmware wires its slave
//! peripheral to, so that every device answers a master the same way the API docs promise.
//!
//! - [I2cSlave]: the register map (pointer bytes, auto-increment, wrap at `len`, page wrap, `read_only`), the answer
//!   queue with the underrun policies of `I2cSlaveUnderrun` (`Fill`, `Stretch { max }` with its deadline, `Nack`),
//!   `I2cSlaveEvent::ReadWaiting`, and one `I2cSlaveTransfer` per transaction.
//! - [SpiSlave]: frame answers from the queue (`idle`, `repeat_last`) or the register map laid out by `SpiRegisters`
//!   (address, `rw` bit, dummy bytes), and one `SpiSlaveFrame` per CS assertion.
//!
//! Both are sans-IO: the firmware calls byte-level hooks from its interrupt handler (an address matched, a byte
//! came, a byte is wanted, a stop; CS edges) and the task side (the API handlers) calls `configure`, the map and
//! queue methods, and drains the events into the `slave_events` / `slave_frames` stream. Times are `u32`
//! microseconds of the device clock (wrapping), the unit the events carry.
//!
//! # Sharing between the interrupt and the task
//!
//! Every method takes `&mut self`; the firmware decides how the interrupt and the task share it, usually a
//! `critical_section::Mutex<RefCell<..>>` (or embassy's `blocking_mutex::Mutex`) in a `static`. `new` is a
//! `const fn`, so the engine is built in place in that `static`, never on the stack:
//!
//! ```
//! use core::cell::RefCell;
//! use ww_bus_slave::I2cSlave;
//! // 256-byte map, 64 bytes of staged host updates, 4 answers of 32 bytes, 8 events of 32 bytes
//! type Slave = I2cSlave<256, 64, 4, 32, 8, 32>;
//! static SLAVE: critical_section_like::Mutex<RefCell<Slave>> =
//!     critical_section_like::Mutex::new(RefCell::new(Slave::new()));
//! # mod critical_section_like {
//! #     pub struct Mutex<T>(T);
//! #     unsafe impl<T> Sync for Mutex<T> {}
//! #     impl<T> Mutex<T> { pub const fn new(t: T) -> Self { Mutex(t) } }
//! # }
//! ```
//!
//! A host update of the map (`map_write`) made while the master is in a transaction is staged and applied at its
//! stop (I2C) or CS release (SPI), so the master never reads half of an update; the engine's own critical
//! sections stay short (a copy of the update at most).
#![no_std]

mod i2c;
mod map;
mod queue;
mod spi;

pub use i2c::{I2cSlave, ReadByte};
pub use map::MapError;
pub use queue::QueueError;
pub use spi::SpiSlave;

use wire_weaver::shrink_wrap::MaxLen;
use ww_error::CommonError;

/// Why a slave config was refused, convertible into the API's `I2cError` / `SpiError`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// A source, policy or layout this engine (as sized by the firmware) doesn't do: no map or no queue, more than
    /// 4 pointer or address bytes, a word size other than 8 bits.
    Unsupported,
    /// The register map is larger than the engine's.
    BufferTooBig,
    /// An argument out of its range, with the reason.
    InvalidArgument(&'static str),
}

impl ConfigError {
    fn common(self) -> Option<CommonError<'static>> {
        match self {
            ConfigError::Unsupported => Some(CommonError::Unsupported),
            ConfigError::BufferTooBig => None,
            ConfigError::InvalidArgument(reason) => Some(CommonError::InvalidArgument {
                arg: 0,
                reason: MaxLen::new_unchecked(reason),
            }),
        }
    }
}

impl From<ConfigError> for ww_i2c::I2cError<'static> {
    fn from(e: ConfigError) -> Self {
        match e.common() {
            Some(c) => ww_i2c::I2cError::Common(c),
            None => ww_i2c::I2cError::BufferTooBig,
        }
    }
}

impl From<ConfigError> for ww_spi::SpiError<'static> {
    fn from(e: ConfigError) -> Self {
        match e.common() {
            Some(c) => ww_spi::SpiError::Common(c),
            None => ww_spi::SpiError::BufferTooBig,
        }
    }
}

impl From<MapError> for ww_i2c::I2cError<'static> {
    fn from(e: MapError) -> Self {
        match e {
            MapError::OutOfRange => ww_i2c::I2cError::BufferTooBig,
            MapError::Busy => ww_i2c::I2cError::Common(CommonError::Busy),
        }
    }
}

impl From<MapError> for ww_spi::SpiError<'static> {
    fn from(e: MapError) -> Self {
        match e {
            MapError::OutOfRange => ww_spi::SpiError::BufferTooBig,
            MapError::Busy => ww_spi::SpiError::Common(CommonError::Busy),
        }
    }
}

/// A record slot of the event ring, written in place while its transfer goes on.
pub(crate) trait Slot {
    const EMPTY: Self;
}

/// `N` record slots: committed records from `head`, then the one being written (`open`). A transfer that finds no
/// free slot isn't recorded and counts as dropped.
pub(crate) struct Ring<T, const N: usize> {
    slots: [T; N],
    head: usize,
    committed: usize,
    /// The slot after the committed ones is being written.
    open: bool,
    /// Its index.
    cur: usize,
    /// The transfer going on found the ring full.
    dropping: bool,
    dropped: u32,
}

impl<T: Slot, const N: usize> Ring<T, N> {
    pub(crate) const fn new() -> Self {
        Ring {
            slots: [const { T::EMPTY }; N],
            head: 0,
            committed: 0,
            open: false,
            cur: 0,
            dropping: false,
            dropped: 0,
        }
    }

    #[inline(always)]
    fn index(&self, k: usize) -> usize {
        let i = self.head + k;
        if i >= N { i - N } else { i }
    }

    /// Starts a record; `None` when the ring is full (the transfer is counted as dropped when it ends).
    #[inline(always)]
    pub(crate) fn open(&mut self) -> Option<&mut T> {
        if self.committed < N {
            self.open = true;
            self.dropping = false;
            self.cur = self.index(self.committed);
            Some(&mut self.slots[self.cur])
        } else {
            self.open = false;
            self.dropping = true;
            None
        }
    }

    /// The record being written.
    #[inline(always)]
    pub(crate) fn current(&mut self) -> Option<&mut T> {
        if self.open {
            Some(&mut self.slots[self.cur])
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn current_ref(&self) -> Option<&T> {
        if self.open {
            Some(&self.slots[self.cur])
        } else {
            None
        }
    }

    /// Ends the record being written: it can be read now.
    #[inline(always)]
    pub(crate) fn commit(&mut self) {
        if self.open {
            self.committed += 1;
            self.open = false;
        } else if self.dropping {
            self.dropped = self.dropped.wrapping_add(1);
            self.dropping = false;
        }
    }

    /// Ends the record being written without keeping it (nothing happened worth reporting).
    pub(crate) fn discard(&mut self) {
        self.open = false;
        self.dropping = false;
    }

    pub(crate) fn oldest(&self) -> Option<&T> {
        if self.committed > 0 {
            Some(&self.slots[self.head])
        } else {
            None
        }
    }

    pub(crate) fn oldest_mut(&mut self) -> Option<&mut T> {
        if self.committed > 0 {
            Some(&mut self.slots[self.head])
        } else {
            None
        }
    }

    pub(crate) fn pop(&mut self) {
        if self.committed > 0 {
            self.head = self.index(1);
            self.committed -= 1;
        }
    }

    pub(crate) fn take_dropped(&mut self) -> u32 {
        core::mem::take(&mut self.dropped)
    }

    pub(crate) fn reset(&mut self) {
        self.head = 0;
        self.committed = 0;
        self.open = false;
        self.dropping = false;
        self.dropped = 0;
    }
}

/// Appends a byte to a record buffer, counting the bytes that don't fit (truncated).
#[inline(always)]
pub(crate) fn record_byte<const B: usize>(buf: &mut [u8; B], count: &mut u32, byte: u8) {
    let i = *count as usize;
    if i < B {
        buf[i] = byte;
    }
    *count += 1;
}
