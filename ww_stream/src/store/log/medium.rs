//! [LogMedium]: what a [LogStore](super::LogStore) needs from the storage under it.

/// Byte-addressed storage under a [LogStore](super::LogStore): an SD card ([SdMedium](super::SdMedium)), NOR flash
/// ([NorMedium](super::NorMedium)) or a simulated device in tests ([SimMedium](super::SimMedium)).
///
/// The store only writes whole units of [WRITE_SIZE](Self::WRITE_SIZE) bytes at offsets that are multiples of it,
/// and only erases whole units of [ERASE_SIZE](Self::ERASE_SIZE). Reads can be any length at any offset.
///
/// Power cuts: on a medium that overwrites ([NEEDS_ERASE](Self::NEEDS_ERASE) `false`), a unit write either happens
/// or doesn't (as SD cards do for a 512-byte block); the store rewrites the unit that holds the end of the log on
/// each push. On a medium that needs erasing, a cut write or erase can leave anything in that unit; the store never
/// writes a unit twice between erases.
#[allow(async_fn_in_trait)]
pub trait LogMedium {
    /// Error of the storage.
    type Error;
    /// Smallest write, bytes.
    const WRITE_SIZE: usize;
    /// Erase unit, bytes: superblock slots and segments are aligned to it when [NEEDS_ERASE](Self::NEEDS_ERASE).
    const ERASE_SIZE: usize;
    /// A written unit has to be erased before it is written again; erased bytes read `0xff` (NOR flash). `false`:
    /// a write replaces what was there (SD card).
    const NEEDS_ERASE: bool;

    /// Bytes the medium holds.
    fn capacity(&self) -> u64;

    /// Read `buf.len()` bytes at `offset`.
    async fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error>;

    /// Write `data` at `offset`, both multiples of [WRITE_SIZE](Self::WRITE_SIZE).
    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), Self::Error>;

    /// Erase `from..to`, both multiples of [ERASE_SIZE](Self::ERASE_SIZE). Called only when
    /// [NEEDS_ERASE](Self::NEEDS_ERASE).
    async fn erase(&mut self, from: u64, to: u64) -> Result<(), Self::Error>;
}
