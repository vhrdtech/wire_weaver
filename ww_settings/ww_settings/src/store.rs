//! [Store]: the records of every group, kept in a range of NOR flash by a `sequential-storage` map.

use core::ops::Range;
use embedded_storage_async::nor_flash::NorFlash;
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapConfigError, MapStorage};

/// Key of a settings group in the [Store]: one record per key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Key(pub u8);

impl Key {
    /// Settings the user changes: reset by a factory reset.
    pub const USER: Key = Key(0);
    /// Data written once at production or calibration (written through a debug API only), kept by a factory
    /// reset.
    pub const FACTORY: Key = Key(1);
}

/// Errors of the [Store].
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StoreError<E> {
    /// From `sequential-storage`: the flash failed, the store is full (the records of all keys don't fit the
    /// pages, see the docs page), or it is corrupted beyond its own repair.
    Map(sequential_storage::Error<E>),
    /// A stored record is longer than the buffer it is read into.
    TooBig,
}

impl<E> From<sequential_storage::Error<E>> for StoreError<E> {
    fn from(e: sequential_storage::Error<E>) -> Self {
        StoreError::Map(e)
    }
}

/// The records of every settings group in `range` of `flash`, one per [Key]: a `sequential-storage` 8 map.
///
/// Pages are written in turn, a record at a time; each write appends a new record for its key, and the newest
/// one wins. When a page is full, the next one (always kept erased) takes over, and the oldest page's records
/// that are still current are copied into it before that page is erased. So the range needs at least 2 pages,
/// and the records of all keys have to fit into one page together with the one being written (see the docs
/// page for the numbers and for flash with big sectors).
///
/// Never writes a flash word twice between erases (STM32G0/H7 ECC flash): items and page markers are whole words
/// written once, and the one call that would overwrite a word (`remove_item`) is never used.
///
/// `B` is the buffer for one item (the key byte and the record, rounded up to the flash word) and for the bytes
/// of one field while a record is put together.
pub struct Store<S: NorFlash, const B: usize = 512> {
    map: MapStorage<u8, S, Cache<Uncached, Uncached, Uncached, u8>>,
    buf: [u8; B],
}

impl<S: NorFlash, const B: usize> Store<S, B> {
    /// A store in `range` (absolute flash addresses as `flash` takes them), aligned to `S::ERASE_SIZE` and at
    /// least 2 pages long.
    pub fn new(flash: S, range: Range<u32>) -> Result<Self, MapConfigError> {
        let config = MapConfig::try_new(range)?;
        Ok(Store {
            map: MapStorage::new(flash, config, Cache::new_uncached()),
            buf: [0; B],
        })
    }

    /// Usable bytes of one page: the page minus its two marker words.
    pub const fn page_capacity() -> usize {
        S::ERASE_SIZE - 2 * word::<S>()
    }

    /// Bytes a stored record of `record_len` takes in a page: the 8-byte item header and the key byte with the
    /// record, each rounded up to the flash word.
    pub const fn footprint(record_len: usize) -> usize {
        let w = word::<S>();
        8usize.div_ceil(w) * w + (1 + record_len).div_ceil(w) * w
    }

    /// Erase the whole range: every group reads as its defaults afterwards. For a new device or a range that held
    /// something else.
    pub async fn erase_all(&mut self) -> Result<(), StoreError<S::Error>> {
        Ok(self.map.erase_all().await?)
    }

    /// The flash under the store.
    pub fn flash(&mut self) -> &mut S {
        self.map.flash()
    }

    /// The flash, the store is gone.
    pub fn into_flash(self) -> S {
        self.map.destroy().0
    }

    /// Read the newest record of `key` into `out`, its length. `None` when none is stored.
    pub async fn fetch(
        &mut self,
        key: crate::Key,
        out: &mut [u8],
    ) -> Result<Option<usize>, StoreError<S::Error>> {
        let Some(bytes) = self.map.fetch_item::<&[u8]>(&mut self.buf, &key.0).await? else {
            return Ok(None);
        };
        let out = out.get_mut(..bytes.len()).ok_or(StoreError::TooBig)?;
        out.copy_from_slice(bytes);
        Ok(Some(bytes.len()))
    }

    /// Append `record` as the newest one of `key`.
    pub async fn store(
        &mut self,
        key: crate::Key,
        record: &[u8],
    ) -> Result<(), StoreError<S::Error>> {
        Ok(self.map.store_item(&mut self.buf, &key.0, &record).await?)
    }

    /// The buffer, for the bytes of one field while a record is put together.
    pub(crate) fn scratch(&mut self) -> &mut [u8] {
        &mut self.buf
    }
}

const fn word<S: NorFlash>() -> usize {
    if S::WRITE_SIZE > S::READ_SIZE {
        S::WRITE_SIZE
    } else {
        S::READ_SIZE
    }
}
