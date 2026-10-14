//! 1-Wire over WireWeaver: a device (a USB adapter, a dev board, an instrument) lets a host reset the bus, find the
//! ROM ids on it, and run addressed transactions against them.
//!
//! # Traits
//!
//! | Trait     | What it is for                                              | Who implements it   |
//! |-----------|--------------------------------------------------------------|---------------------|
//! | `OneWire` | Reset, ROM search (paged or bit-level), addressed transactions, bus configuration. | Every 1-Wire master |
//!
//! ```rust,ignore
//! #[ww_api_root]
//! trait MyAdapter {
//!     ww_impl!(onewire: ww_onewire::OneWire);
//! }
//! ```
//!
//! # The protocol, briefly
//!
//! A 1-Wire bus is a single data line shared by a master and one or more slaves, each identified by an 8-byte
//! [RomId]: a family code, a 48-bit serial number, and a CRC8 of the other seven bytes. Every operation starts with
//! a reset pulse and a presence pulse from any slave listening (`OneWire::reset`); `OneWire::transaction` does this
//! implicitly. A slave is picked with its [RomId] (`Match ROM`) or, on a bus with one device, skipped entirely
//! (`rom: None`, `Skip ROM`).
//!
//! Finding the [RomId]s on a bus (there is no broadcast "who's there") is a bit-level search: a reset, the `Search
//! ROM` command (`0xF0`) written through `OneWire::transaction` with an empty read, then up to 64 calls to
//! `OneWire::triplet`, one per ROM bit, walking a binary tree of every bit position where connected devices
//! disagree. `OneWire::search` runs this on the device and returns whole ids a page at a time, which is what most
//! hosts want; `OneWire::triplet` exposes the same primitive for a host that wants to drive the tree walk itself
//! (e.g. to stop early once it has found the family code it is looking for).
//!
//! # Strong pull-up
//!
//! Parasite-powered parts (no separate Vcc pin) draw their supply from the data line and need it actively driven
//! high for the duration of certain operations (EEPROM writes, DS18B20 temperature conversions) rather than left to
//! the bus pull-up resistor. `OneWireTransaction::strong_pullup` asks the device to do this for the given duration
//! right after the transaction's last byte; [OneWireCapabilities::strong_pullup] says whether it can.
//!
//! # Errors
//!
//! Every method returns [OneWireError]; each lists the variants it returns.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::Second;

/// Largest write or read of one transaction, in bytes.
pub const OW_MAX: usize = 256;

/// Most ROM ids in one search page.
pub const MAX_ROMS: usize = 16;

/// Most pins a bus can be put on.
pub const MAX_PINS: usize = 8;

/// A 1-Wire master: reset the bus, find the devices on it, and run addressed transactions.
#[ww_trait]
pub trait OneWire {
    /// Reset, ROM select (or skip ROM), write, read `read_len` bytes, optional strong pull-up after the last byte.
    ///
    /// # Errors
    ///
    /// [OneWireError::NoPresence] (the reset found no device), [OneWireError::ShortCircuit],
    /// [OneWireError::InUse] (another transaction or search is running), [OneWireError::WrongMode] (the bus is not
    /// configured, see `OneWire::configure`), [OneWireError::NoIoLevel],
    /// [OneWireError::InvalidArgument] (`write.len() + read_len` over [OW_MAX], or `strong_pullup` on a device
    /// without [OneWireCapabilities::strong_pullup]), [OneWireError::Refused].
    fn transaction(
        t: OneWireTransaction<'i>,
    ) -> Result<MaxLen<RefVec<'i, u8>, OW_MAX>, OneWireError>;

    /// Reset pulse; true when a device answered with a presence pulse.
    ///
    /// # Errors
    ///
    /// [OneWireError::ShortCircuit], [OneWireError::InUse], [OneWireError::WrongMode], [OneWireError::NoIoLevel].
    fn reset() -> Result<bool, OneWireError>;

    /// ROM search, one page at a time: start with `from: None`, continue with the page's `next` until it comes back
    /// `None`. `family`, when given, only returns ids whose first byte matches.
    ///
    /// # Errors
    ///
    /// [OneWireError::NoPresence] (nothing answered the reset before this page's search), [OneWireError::ShortCircuit],
    /// [OneWireError::InUse], [OneWireError::WrongMode], [OneWireError::NoIoLevel],
    /// [OneWireError::CrcMismatch] (a found id's own CRC8 did not check out).
    fn search(
        from: Option<SearchState>,
        family: Option<u8>,
    ) -> Result<SearchPage<'i>, OneWireError>;

    /// What this bus supports: pins it can be put on, overdrive, strong pull-up.
    fn capabilities() -> OneWireCapabilities<'i>;

    /// Puts the bus on one of [OneWireCapabilities::pins] at the given speed, or disables it (`enabled: false`).
    ///
    /// # Errors
    ///
    /// [OneWireError::InvalidArgument] (`pin` outside [OneWireCapabilities::pins]), [OneWireError::Unsupported]
    /// (`speed: Overdrive` without [OneWireCapabilities::overdrive]), [OneWireError::InUse] (the pin serves another
    /// function), [OneWireError::Refused] (a transaction or search is running).
    fn configure(config: OneWireConfig) -> Result<(), OneWireError>;

    /// One search step at bit level, for a search run on the host: after `OneWire::reset` and writing the `Search
    /// ROM` command (`0xF0`) through `OneWire::transaction`, call this up to 64 times, once per ROM bit. `id_bit`
    /// and `cmp_bit` are read back from the bus for this bit position (both set when every remaining device agrees
    /// on it, both clear at a fork between devices); `direction` is the bit this call actually drove onto the bus
    /// for the next step (the agreed bit when there is no fork, the caller's `direction` at a fork).
    ///
    /// # Errors
    ///
    /// [OneWireError::WrongMode] (no `Search ROM` command was sent since the last reset, or more than 64 calls
    /// since), [OneWireError::ShortCircuit], [OneWireError::NoIoLevel], [OneWireError::Refused].
    fn triplet(direction: bool) -> Result<Triplet, OneWireError>;
}

/// A 1-Wire ROM id: an 8-bit family code, a 48-bit serial number, and a CRC8 of the other seven bytes
/// (`bytes[0..7]`), most to least significant as read off the bus.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct RomId {
    pub bytes: [u8; 8],
}

/// One addressed (or skip-ROM) transaction: reset, select, write, read, optional strong pull-up.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct OneWireTransaction<'i> {
    /// `None` = skip ROM (one device on the bus).
    pub rom: Option<RomId>,
    pub write: MaxLen<RefVec<'i, u8>, OW_MAX>,
    pub read_len: u16,
    /// Drive the bus high this long after the last byte (parasite-powered parts converting or writing).
    pub strong_pullup: Option<Second>,
}

/// Where a paged `OneWire::search` left off: the classic Dallas/Maxim search state, carried from one page to the
/// next so the device doesn't have to remember it between calls.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SearchState {
    /// The last ROM id found, continuing the tree walk from the bit after it.
    pub last_rom: RomId,
    /// Bit position (1-64, counting from the first bit sent) of the last fork where the search took the 0-branch
    /// while a 1-branch was still open; the next page explores that 1-branch first.
    pub last_discrepancy: u8,
}

/// One page of `OneWire::search` results.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SearchPage<'i> {
    pub roms: MaxLen<RefVec<'i, RomId>, MAX_ROMS>,
    /// Where to continue, `None` when all devices were found.
    pub next: Option<SearchState>,
}

/// Result of one `OneWire::triplet` step.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct Triplet {
    /// This bit position, as driven by the device(s) still on the bus.
    pub id_bit: bool,
    /// Complement of `id_bit`, driven right after it; equal to `id_bit` at a fork between devices.
    pub cmp_bit: bool,
    /// The bit this step actually selected (sent back onto the bus for the devices to compare against).
    pub direction: bool,
}

/// Bus pin and speed, set with `OneWire::configure`.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct OneWireConfig {
    pub enabled: bool,
    /// Index into [OneWireCapabilities::pins].
    pub pin: u8,
    pub speed: OneWireSpeed,
}

/// 1-Wire bus timing.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum OneWireSpeed {
    Standard,
    Overdrive,
}

/// What a bus supports, see `OneWire::capabilities`.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct OneWireCapabilities<'i> {
    /// Pins the bus can be put on, by name.
    pub pins: MaxLen<RefVec<'i, MaxLen<&'i str, 16>>, MAX_PINS>,
    pub overdrive: bool,
    pub strong_pullup: bool,
}

/// Errors every `OneWire` method returns a subset of, see each method's `# Errors`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum OneWireError {
    /// The requested capability is not there (e.g. overdrive without [OneWireCapabilities::overdrive]).
    Unsupported,
    /// The pin, or the bus itself, is already serving another function.
    InUse,
    /// No supply detected for the bus pull-up.
    NoIoLevel,
    /// A valid request the device declines right now.
    Refused,
    /// Bad argument (an out-of-range pin index, a transaction over [OW_MAX]).
    InvalidArgument,
    /// The bus is not in the state this call needs (not configured, or a bit-level search wasn't started).
    WrongMode,
    /// No device answered the reset.
    NoPresence,
    /// A ROM id's own CRC8 did not check out.
    CrcMismatch,
    /// The bus is held low.
    ShortCircuit,
}
