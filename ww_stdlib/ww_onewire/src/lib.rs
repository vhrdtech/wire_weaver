//! 1-Wire over WireWeaver: reset, ROM search, addressed transactions with strong pull-up.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_si::Second;

/// Largest write or read of one transaction, in bytes.
pub const OW_MAX: usize = 256;

/// Most ROM ids in one search page.
pub const MAX_ROMS: usize = 16;

/// Most pins a bus can be put on.
pub const MAX_PINS: usize = 8;

#[ww_trait]
pub trait OneWire {
    /// Reset, ROM select (or skip ROM), write, read `read_len` bytes, optional strong pull-up after the last byte.
    fn transaction(
        t: OneWireTransaction<'i>,
    ) -> Result<MaxLen<RefVec<'i, u8>, OW_MAX>, OneWireError>;

    /// Reset pulse; true when a device answered with a presence pulse.
    fn reset() -> Result<bool, OneWireError>;

    /// ROM search, one page at a time: start with `from: None`, continue with the page's `next`.
    fn search(
        from: Option<SearchState>,
        family: Option<u8>,
    ) -> Result<SearchPage<'i>, OneWireError>;

    fn capabilities() -> OneWireCapabilities<'i>;

    fn configure(config: OneWireConfig) -> Result<(), OneWireError>;

    /// One search step at bit level, for a search run on the host.
    fn triplet(direction: bool) -> Result<Triplet, OneWireError>;
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct RomId {
    pub bytes: [u8; 8],
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct OneWireTransaction<'i> {
    /// `None` = skip ROM (one device on the bus).
    pub rom: Option<RomId>,
    pub write: MaxLen<RefVec<'i, u8>, OW_MAX>,
    pub read_len: u16,
    /// Drive the bus high this long after the last byte (parasite-powered parts converting).
    pub strong_pullup: Option<Second>,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct SearchState {
    pub last_rom: RomId,
    pub last_discrepancy: u8,
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct SearchPage<'i> {
    pub roms: MaxLen<RefVec<'i, RomId>, MAX_ROMS>,
    /// Where to continue, `None` when all devices were found.
    pub next: Option<SearchState>,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct Triplet {
    pub id_bit: bool,
    pub cmp_bit: bool,
    pub direction: bool,
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug))]
pub struct OneWireConfig {
    pub enabled: bool,
    /// Index into [OneWireCapabilities::pins].
    pub pin: u8,
    pub speed: OneWireSpeed,
}

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

#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug))]
pub struct OneWireCapabilities<'i> {
    /// Pins the bus can be put on, by name.
    pub pins: MaxLen<RefVec<'i, MaxLen<&'i str, 16>>, MAX_PINS>,
    pub overdrive: bool,
    pub strong_pullup: bool,
}

#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum OneWireError {
    Unsupported,
    InUse,
    NoIoLevel,
    Refused,
    InvalidArgument,
    WrongMode,
    /// No device answered the reset.
    NoPresence,
    CrcMismatch,
    /// The bus is held low.
    ShortCircuit,
}
