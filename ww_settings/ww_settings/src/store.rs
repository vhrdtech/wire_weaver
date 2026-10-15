//! [Store]: the records of every settings group in a range of NOR flash, a small log with a sequence number per
//! key, safe against a power cut at any point of a write or an erase.
//!
//! # Layout
//!
//! The range is cut into pages (the flash's erase unit, `ERASE_SIZE`). A page in use starts with a header, then
//! items one after another, each padded to the flash word (`W`, the larger of `WRITE_SIZE` and `READ_SIZE`) with
//! `0xff`:
//!
//! ```text
//! page header:  | 'W' 'S' 1 0xff | page_seq: u32 LE | crc32 of the 8 bytes before: u32 LE |
//! item:         | len: u16 LE | key: u8 | 0xa5 | seq: u32 LE | crc32: u32 LE | data: len bytes |
//! ```
//!
//! The item's crc32 (Castagnoli) covers its first 8 bytes and the data. `seq` counts the writes of the key: the
//! valid item with the highest `seq` is the key's current record, wherever it is (on a tie, the copy on the page
//! with the higher `page_seq`). The current page is the one with the highest valid `page_seq`.
//!
//! # Writing and compacting
//!
//! Items are appended to the current page. When the next one doesn't fit, the store moves on to the page after
//! it: erases it if it isn't erased (it never holds a current record), writes its header with the next
//! `page_seq`, and copies into it the current records that sit on the page after that one, so that page holds
//! none and can be erased later. Then the item is written. So the current records of all keys, together with the
//! item being written, have to fit into one page.
//!
//! The erase can be done ahead with [Store::maintain], when a stall suits the device (on a single-bank STM32F4 or
//! H7 an erase stops the CPU for up to seconds).
//!
//! # Power cuts
//!
//! - A cut item write leaves an item that fails its crc32: it is skipped a word at a time, its space is lost until
//!   that page is erased, and the key's previous record is current.
//! - A cut page header write: the page is not used, and erased again before it is.
//! - A cut erase: every record still current on that page was copied away before the erase started, so whatever
//!   the erase left, in whatever order it cleared the bytes, is older than the current records or a copy of one.
//! - A cut while copying: the page copied from is untouched; the next write finishes the copy first.
//! - Loading never writes: a device that boots in a loop doesn't wear or erase anything.
//! - A write or [maintain](Store::maintain) future dropped half way (a `select!` with a timeout) leaves the flash
//!   as a power cut at that point would, and the store goes on from there.
//!
//! Every word is written once between erases, as STM32G0 / H7 ECC flash needs, and only words that read erased
//! are written. A page that holds a current record is never erased.

use core::ops::Range;
use crc::{CRC_32_ISCSI, Crc};
use embedded_storage_async::nor_flash::NorFlash;

pub(crate) const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISCSI);
pub(crate) const PAGE_MAGIC: [u8; 4] = [b'W', b'S', 1, 0xff];
pub(crate) const PAGE_HEADER_LEN: usize = 12;
pub(crate) const ITEM_TAG: u8 = 0xa5;
pub(crate) const ITEM_HEADER_LEN: usize = 12;
/// Largest flash word supported (STM32H7: 32 bytes).
const MAX_WORD: usize = 32;

/// Key of a settings group in the [Store]: one record per key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Key(pub u8);

impl Key {
    /// Settings the user changes: reset by a factory reset.
    pub const USER: Key = Key(0);
    /// Data written once at production or calibration (through a debug API only), kept by a factory reset.
    pub const FACTORY: Key = Key(1);
}

/// Why a [Store] can't be made for a range.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// The range doesn't start and end on a page boundary.
    NotAligned,
    /// Fewer than 2 pages: one is always kept free to compact into.
    TooFewPages,
    /// The flash word is larger than 32 bytes, or `READ_SIZE` doesn't divide it.
    UnsupportedWord,
    /// `B` is smaller than 32 bytes or not a multiple of the flash word.
    BufferTooSmall,
}

/// Errors of the [Store].
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StoreError<E> {
    /// The flash failed.
    Flash(E),
    /// The current records of all keys and the new one don't fit one page. Nothing was written.
    Full,
    /// A record is longer than the buffer it is read into, or longer than a page.
    TooBig,
    /// More keys than the store tracks (`K`).
    TooManyKeys,
}

/// Where a key's current record is.
#[derive(Copy, Clone, Debug)]
struct Entry {
    key: u8,
    seq: u32,
    page_seq: u32,
    addr: u32,
    len: u16,
}

/// The state found by a scan, kept up to date by writes.
#[derive(Copy, Clone, Debug)]
struct State {
    /// Index and `page_seq` of the current page, `None` when no page has a valid header.
    current: Option<(usize, u32)>,
    /// First offset in the current page from which every byte reads erased.
    free: usize,
}

/// The records of every settings group in `range` of `flash`, one current record per [Key]; see the
/// [module docs](self) for the layout and how it survives power cuts.
///
/// `B` is a buffer for reading and writing items a chunk at a time and for the bytes of one field while a record
/// is put together (at least 32 bytes and a multiple of the flash word); `K` is how many keys it tracks.
pub struct Store<S: NorFlash, const B: usize = 512, const K: usize = 8> {
    flash: S,
    range: Range<u32>,
    buf: [u8; B],
    entries: [Option<Entry>; K],
    state: Option<State>,
}

impl<S: NorFlash, const B: usize, const K: usize> Store<S, B, K> {
    const W: usize = if S::WRITE_SIZE > S::READ_SIZE {
        S::WRITE_SIZE
    } else {
        S::READ_SIZE
    };
    const P: usize = S::ERASE_SIZE;
    const PAGE_HEADER: usize = PAGE_HEADER_LEN.div_ceil(Self::W) * Self::W;

    /// A store in `range` (absolute flash addresses as `flash` takes them), aligned to pages and at least 2 pages
    /// long. Nothing is read until the first [fetch](Self::fetch) or [store](Self::store).
    pub fn new(flash: S, range: Range<u32>) -> Result<Self, ConfigError> {
        if Self::W > MAX_WORD || !Self::W.is_multiple_of(S::READ_SIZE) {
            return Err(ConfigError::UnsupportedWord);
        }
        if B < 32 || !B.is_multiple_of(Self::W) {
            return Err(ConfigError::BufferTooSmall);
        }
        let (start, end) = (range.start as usize, range.end as usize);
        if !start.is_multiple_of(Self::P) || !end.is_multiple_of(Self::P) || end < start {
            return Err(ConfigError::NotAligned);
        }
        if (end - start) / Self::P < 2 {
            return Err(ConfigError::TooFewPages);
        }
        Ok(Store {
            flash,
            range,
            buf: [0; B],
            entries: [None; K],
            state: None,
        })
    }

    /// Bytes of a page that items can use.
    pub const fn page_capacity() -> usize {
        Self::P - Self::PAGE_HEADER
    }

    /// Bytes a record of `record_len` takes in a page: the item header and the record, padded to the flash word.
    pub const fn footprint(record_len: usize) -> usize {
        (ITEM_HEADER_LEN + record_len).div_ceil(Self::W) * Self::W
    }

    /// The flash under the store. Changing its contents behind the store's back: call
    /// [forget](Self::forget) afterwards.
    pub fn flash(&mut self) -> &mut S {
        &mut self.flash
    }

    /// The flash, the store is gone.
    pub fn into_flash(self) -> S {
        self.flash
    }

    /// Forget what was read: the next operation reads the range again.
    pub fn forget(&mut self) {
        self.state = None;
    }

    fn pages(&self) -> usize {
        (self.range.end - self.range.start) as usize / Self::P
    }

    fn page_addr(&self, page: usize) -> u32 {
        self.range.start + (page * Self::P) as u32
    }

    fn page_range(&self, page: usize) -> Range<u32> {
        self.page_addr(page)..self.page_addr(page) + Self::P as u32
    }

    fn next_page(&self, page: usize) -> usize {
        (page + 1) % self.pages()
    }

    async fn read(&mut self, addr: u32, len: usize) -> Result<(), StoreError<S::Error>> {
        self.flash
            .read(addr, &mut self.buf[..len])
            .await
            .map_err(StoreError::Flash)
    }

    /// The `page_seq` of a page with a valid header.
    async fn page_seq(&mut self, page: usize) -> Result<Option<u32>, StoreError<S::Error>> {
        self.read(self.page_addr(page), Self::PAGE_HEADER).await?;
        let h = &self.buf[..PAGE_HEADER_LEN];
        if h[..4] != PAGE_MAGIC || CRC.checksum(&h[..8]).to_le_bytes() != h[8..12] {
            return Ok(None);
        }
        Ok(Some(u32::from_le_bytes(h[4..8].try_into().unwrap())))
    }

    /// Every byte of `range` reads erased.
    async fn is_erased(&mut self, range: Range<u32>) -> Result<bool, StoreError<S::Error>> {
        let mut addr = range.start;
        while addr < range.end {
            let n = ((range.end - addr) as usize).min(B);
            self.read(addr, n).await?;
            if self.buf[..n].iter().any(|b| *b != 0xff) {
                return Ok(false);
            }
            addr += n as u32;
        }
        Ok(true)
    }

    /// The valid item at `addr`, which ends before `end`: `(len, key, seq)`, its crc checked.
    async fn item_at(
        &mut self,
        addr: u32,
        end: u32,
    ) -> Result<Option<(u16, u8, u32)>, StoreError<S::Error>> {
        let first = ITEM_HEADER_LEN.div_ceil(Self::W) * Self::W;
        if addr + first as u32 > end {
            return Ok(None);
        }
        self.read(addr, first).await?;
        let h: [u8; ITEM_HEADER_LEN] = self.buf[..ITEM_HEADER_LEN].try_into().unwrap();
        if h[3] != ITEM_TAG {
            return Ok(None);
        }
        let len = u16::from_le_bytes([h[0], h[1]]) as usize;
        let size = Self::footprint(len);
        if addr as usize + size > end as usize {
            return Ok(None);
        }
        // crc over bytes 0..8 and the data, read a chunk at a time from the item's start
        let mut digest = CRC.digest();
        digest.update(&h[..8]);
        let data = ITEM_HEADER_LEN..ITEM_HEADER_LEN + len;
        let mut at = 0;
        while at < data.end {
            let n = (size - at).min(B);
            self.read(addr + at as u32, n).await?;
            let from = data.start.max(at);
            let to = data.end.min(at + n);
            if to > from {
                digest.update(&self.buf[from - at..to - at]);
            }
            at += n;
        }
        if digest.finalize().to_le_bytes() != h[8..12] {
            return Ok(None);
        }
        Ok(Some((
            len as u16,
            h[2],
            u32::from_le_bytes(h[4..8].try_into().unwrap()),
        )))
    }

    /// The first word at or after `addr` (word aligned) that doesn't read erased, `end` if none.
    async fn skip_erased(&mut self, mut addr: u32, end: u32) -> Result<u32, StoreError<S::Error>> {
        while addr < end {
            let n = ((end - addr) as usize).min(B);
            self.read(addr, n).await?;
            if let Some(i) = self.buf[..n].iter().position(|b| *b != 0xff) {
                return Ok(addr + (i / Self::W * Self::W) as u32);
            }
            addr += n as u32;
        }
        Ok(end)
    }

    /// Read the range once: the current page, its free space, the current record of each key. Reads only.
    async fn scan(&mut self) -> Result<State, StoreError<S::Error>> {
        if let Some(s) = self.state {
            return Ok(s);
        }
        self.entries = [None; K];
        let mut current: Option<(usize, u32)> = None;
        for page in 0..self.pages() {
            let Some(page_seq) = self.page_seq(page).await? else {
                continue;
            };
            if current.is_none_or(|(_, s)| page_seq > s) {
                current = Some((page, page_seq));
            }
            let end = self.page_range(page).end;
            let mut addr = self.page_addr(page) + Self::PAGE_HEADER as u32;
            while addr < end {
                match self.item_at(addr, end).await? {
                    Some((len, key, seq)) => {
                        self.notice(Entry {
                            key,
                            seq,
                            page_seq,
                            addr,
                            len,
                        })?;
                        addr += Self::footprint(len as usize) as u32;
                    }
                    // cut or garbage: a word further; erased: to the next word that isn't
                    None => addr = self.skip_erased(addr + Self::W as u32, end).await?,
                }
            }
        }
        let free = match current {
            Some((page, _)) => {
                // after the last word that doesn't read erased
                let r = self.page_range(page);
                let mut free = Self::P;
                while free > Self::PAGE_HEADER {
                    let n = (free - Self::PAGE_HEADER).min(B);
                    let from = r.start + (free - n) as u32;
                    self.read(from, n).await?;
                    match self.buf[..n].iter().rposition(|b| *b != 0xff) {
                        Some(i) => {
                            free = free - n + (i / Self::W + 1) * Self::W;
                            break;
                        }
                        None => free -= n,
                    }
                }
                free
            }
            None => 0,
        };
        let s = State { current, free };
        self.state = Some(s);
        Ok(s)
    }

    /// Take `e` as its key's current record if it is newer.
    fn notice(&mut self, e: Entry) -> Result<(), StoreError<S::Error>> {
        if let Some(slot) = self.entries.iter_mut().flatten().find(|x| x.key == e.key) {
            if (e.seq, e.page_seq) >= (slot.seq, slot.page_seq) {
                *slot = e;
            }
            return Ok(());
        }
        let free = self
            .entries
            .iter_mut()
            .find(|x| x.is_none())
            .ok_or(StoreError::TooManyKeys)?;
        *free = Some(e);
        Ok(())
    }

    /// Footprint of the current records on `page`, but the one of `skip`, 0: none.
    fn current_on(&self, page: usize, skip: Option<u8>) -> usize {
        let r = self.page_range(page);
        self.entries
            .iter()
            .flatten()
            .filter(|e| r.contains(&e.addr) && Some(e.key) != skip)
            .map(|e| Self::footprint(e.len as usize))
            .sum()
    }

    /// Read the current record of `key` into `out`, its length. `None` when none is stored. Reads only.
    pub async fn fetch(
        &mut self,
        key: Key,
        out: &mut [u8],
    ) -> Result<Option<usize>, StoreError<S::Error>> {
        self.scan().await?;
        let Some(e) = self
            .entries
            .iter()
            .flatten()
            .find(|e| e.key == key.0)
            .copied()
        else {
            return Ok(None);
        };
        let out = out.get_mut(..e.len as usize).ok_or(StoreError::TooBig)?;
        self.flash
            .read(e.addr + ITEM_HEADER_LEN as u32, out)
            .await
            .map_err(StoreError::Flash)?;
        Ok(Some(e.len as usize))
    }

    /// Append `record` as the new current record of `key`. Once this returns `Ok`, it survives a power cut; on an
    /// error the previous record is current, also after a cut.
    pub async fn store(&mut self, key: Key, record: &[u8]) -> Result<(), StoreError<S::Error>> {
        let r = self.store_inner(key, record).await;
        if r.is_err() {
            // read the flash again rather than trust what was assumed about a write that failed
            self.state = None;
        }
        r
    }

    async fn store_inner(&mut self, key: Key, record: &[u8]) -> Result<(), StoreError<S::Error>> {
        let size = Self::footprint(record.len());
        if record.len() > u16::MAX as usize || size > Self::page_capacity() {
            return Err(StoreError::TooBig);
        }
        let s = self.scan().await?;
        let known = self
            .entries
            .iter()
            .flatten()
            .find(|e| e.key == key.0)
            .copied();
        if known.is_none() && self.entries.iter().all(|e| e.is_some()) {
            return Err(StoreError::TooManyKeys);
        }
        let mut current = match s.current {
            Some(c) => c,
            None => {
                // nothing readable written yet: start on page 0
                self.open_page(0, 1).await?;
                (0, 1)
            }
        };
        // a cut while copying left current records on the next page: copy the rest before anything else (the
        // record of `key` too, the write below might fail)
        let spare = self.next_page(current.0);
        if self.current_on(spare, None) > 0 {
            self.copy_records(spare, current, None).await?;
        }
        if self.state.unwrap().free + size > Self::P {
            current = self.rollover(current, size, key.0).await?;
        }
        let seq = known.map_or(1, |e| e.seq.wrapping_add(1));
        self.append(current, key.0, seq, record).await
    }

    /// Erase `page` if it doesn't read erased and make it the current page.
    async fn open_page(&mut self, page: usize, page_seq: u32) -> Result<(), StoreError<S::Error>> {
        if self.current_on(page, None) > 0 {
            // never: the page after the current one holds no current record once copied
            return Err(StoreError::Full);
        }
        let r = self.page_range(page);
        if !self.is_erased(r.clone()).await? {
            self.flash
                .erase(r.start, r.end)
                .await
                .map_err(StoreError::Flash)?;
        }
        let n = Self::PAGE_HEADER;
        self.buf[..n].fill(0xff);
        self.buf[..4].copy_from_slice(&PAGE_MAGIC);
        self.buf[4..8].copy_from_slice(&page_seq.to_le_bytes());
        let crc = CRC.checksum(&self.buf[..8]);
        self.buf[8..12].copy_from_slice(&crc.to_le_bytes());
        self.flash
            .write(r.start, &self.buf[..n])
            .await
            .map_err(StoreError::Flash)?;
        // only now: a write future dropped before this point leaves the store as a power cut would
        self.state = Some(State {
            current: Some((page, page_seq)),
            free: n,
        });
        Ok(())
    }

    /// Move to the page after `current`, carrying over the current records of the page after that one, but the
    /// one of `key`, which is about to be replaced (if the write is cut, the next write carries it over first).
    async fn rollover(
        &mut self,
        current: (usize, u32),
        size: usize,
        key: u8,
    ) -> Result<(usize, u32), StoreError<S::Error>> {
        let next = self.next_page(current.0);
        let after = self.next_page(next);
        // with 2 pages `after` is the current page: every current record moves
        if self.current_on(after, Some(key)) + size > Self::page_capacity() {
            return Err(StoreError::Full);
        }
        let next = (next, current.1.wrapping_add(1));
        self.open_page(next.0, next.1).await?;
        self.copy_records(after, next, Some(key)).await?;
        Ok(next)
    }

    /// Copy the current records that sit on page `from`, but the one of `skip`, to the end of the current page
    /// `to`, byte for byte.
    async fn copy_records(
        &mut self,
        from: usize,
        to: (usize, u32),
        skip: Option<u8>,
    ) -> Result<(), StoreError<S::Error>> {
        let r = self.page_range(from);
        for i in 0..K {
            let Some(e) = self.entries[i] else { continue };
            if !r.contains(&e.addr) || Some(e.key) == skip {
                continue;
            }
            let size = Self::footprint(e.len as usize);
            let free = self.state.unwrap().free;
            if free + size > Self::P {
                return Err(StoreError::Full);
            }
            let dst = self.page_addr(to.0) + free as u32;
            self.state.as_mut().unwrap().free += size;
            let mut at = 0;
            while at < size {
                let n = (size - at).min(B);
                self.read(e.addr + at as u32, n).await?;
                self.flash
                    .write(dst + at as u32, &self.buf[..n])
                    .await
                    .map_err(StoreError::Flash)?;
                at += n;
            }
            self.notice(Entry {
                page_seq: to.1,
                addr: dst,
                ..e
            })?;
        }
        Ok(())
    }

    /// Write an item with `data` at the free offset of the current page.
    async fn append(
        &mut self,
        page: (usize, u32),
        key: u8,
        seq: u32,
        data: &[u8],
    ) -> Result<(), StoreError<S::Error>> {
        let size = Self::footprint(data.len());
        let mut h = [0u8; ITEM_HEADER_LEN];
        h[..2].copy_from_slice(&(data.len() as u16).to_le_bytes());
        h[2] = key;
        h[3] = ITEM_TAG;
        h[4..8].copy_from_slice(&seq.to_le_bytes());
        let mut digest = CRC.digest();
        digest.update(&h[..8]);
        digest.update(data);
        h[8..12].copy_from_slice(&digest.finalize().to_le_bytes());

        let addr = self.page_addr(page.0) + self.state.unwrap().free as u32;
        // the space is used from here on, also if the write is cut
        self.state.as_mut().unwrap().free += size;
        // the item a buffer at a time, each chunk whole words
        let mut at = 0;
        while at < size {
            let n = (size - at).min(B);
            for (i, b) in self.buf[..n].iter_mut().enumerate() {
                let pos = at + i;
                *b = if pos < ITEM_HEADER_LEN {
                    h[pos]
                } else {
                    data.get(pos - ITEM_HEADER_LEN).copied().unwrap_or(0xff)
                };
            }
            self.flash
                .write(addr + at as u32, &self.buf[..n])
                .await
                .map_err(StoreError::Flash)?;
            at += n;
        }
        self.notice(Entry {
            key,
            seq,
            page_seq: page.1,
            addr,
            len: data.len() as u16,
        })
    }

    /// Erase the page the next compaction moves to, if it needs it: the slow part of a write that starts a new
    /// page (up to seconds on a single-bank STM32F4 / H7), done when it suits the device. Returns whether it erased.
    /// Not needed for correctness, a write erases it when it has to.
    pub async fn maintain(&mut self) -> Result<bool, StoreError<S::Error>> {
        let s = self.scan().await?;
        let Some((current, _)) = s.current else {
            return Ok(false);
        };
        let next = self.next_page(current);
        if self.current_on(next, None) > 0 {
            // a cut while copying: the next write copies the rest first
            return Ok(false);
        }
        let r = self.page_range(next);
        if self.is_erased(r.clone()).await? {
            return Ok(false);
        }
        self.flash
            .erase(r.start, r.end)
            .await
            .map_err(StoreError::Flash)?;
        Ok(true)
    }

    /// Whether the next write that starts a new page will erase one first (what [maintain](Self::maintain)
    /// would do now).
    pub async fn erase_pending(&mut self) -> Result<bool, StoreError<S::Error>> {
        let s = self.scan().await?;
        let Some((current, _)) = s.current else {
            return Ok(false);
        };
        let r = self.page_range(self.next_page(current));
        Ok(!self.is_erased(r).await?)
    }

    /// Erase the whole range: every group reads as its defaults afterwards. For a new device or a range that held
    /// something else.
    pub async fn erase_all(&mut self) -> Result<(), StoreError<S::Error>> {
        self.state = None;
        self.flash
            .erase(self.range.start, self.range.end)
            .await
            .map_err(StoreError::Flash)
    }

    /// The buffer, for the bytes of one field while a record is put together.
    pub(crate) fn scratch(&mut self) -> &mut [u8] {
        &mut self.buf
    }
}
