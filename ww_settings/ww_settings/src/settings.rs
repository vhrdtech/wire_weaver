//! [Settings]: one group's value in RAM, what is stored of it, and when to write it.

use crate::group::{SettingsGroup, Version};
use crate::record::{self, DecodeStats, EncodeStats, Record, RecordError};
use crate::store::{Key, Store, StoreError};
use embedded_storage_async::nor_flash::NorFlash;

/// When a changed value is written: once it was left alone for `quiet_ms`, or `max_ms` after the first change
/// that isn't stored yet, whichever comes first. A slider dragged for a minute is written every `max_ms`, not on
/// every step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Debounce {
    pub quiet_ms: u32,
    pub max_ms: u32,
}

impl Default for Debounce {
    /// 1 s quiet, at most 10 s.
    fn default() -> Self {
        Debounce {
            quiet_ms: 1_000,
            max_ms: 10_000,
        }
    }
}

/// What [Settings::load] found.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct LoadReport {
    /// A record was stored for the key; `false`: every field is at its default.
    pub found: bool,
    /// Version that wrote the record.
    pub writer: Option<Version>,
    /// Highest version whose data is in the record; above this firmware's after a downgrade.
    pub newest: Option<Version>,
    /// What was in the record.
    pub stats: DecodeStats,
    /// The record is of an unknown (newer) format or doesn't decode: every field is at its default, and the
    /// record is not overwritten until [Settings::reset_to_defaults] (see [Settings::is_read_only]).
    pub unreadable: Option<RecordError>,
}

/// What [Settings::persist] did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Persisted {
    /// A new record was written.
    Written(EncodeStats),
    /// The record would be the same as the stored one: nothing written.
    Unchanged,
}

/// Errors of [Settings].
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<E> {
    /// The flash store failed.
    Store(StoreError<E>),
    /// The stored record can't be read by this firmware, writing would destroy it: see [Settings::is_read_only].
    ReadOnly,
    /// The record doesn't fit `N` bytes (or a field doesn't fit the store's buffer).
    TooBig(shrink_wrap::Error),
}

impl<E> From<StoreError<E>> for Error<E> {
    fn from(e: StoreError<E>) -> Self {
        Error::Store(e)
    }
}

/// One settings group: its value in RAM, the bytes of its stored record (up to `N`), and whether it has to be
/// written.
///
/// Changing a field ([set](Self::set)) only changes RAM; [persist_if_due](Self::persist_if_due) writes it after
/// the [Debounce], [persist](Self::persist) right away. A write happens only when the record changed, so writing
/// the same value again costs nothing.
///
/// `N` is the largest record: the header (2 to 13 bytes) plus, for each field not at its default, its bytes and a
/// length nibble or two. `N` + the key byte must also fit a flash page with room to spare, see the docs page.
pub struct Settings<G: SettingsGroup, const N: usize> {
    key: Key,
    value: G,
    debounce: Debounce,
    /// The stored record, `stored_len` bytes, 0: none stored
    stored: [u8; N],
    stored_len: usize,
    next: [u8; N],
    /// The stored record can't be read, don't overwrite it
    read_only: bool,
    /// Write even an empty record (after a reset)
    force: bool,
    /// `seq` of the newest record of the key, 0: none
    seq: u32,
    /// Time of the first change not written yet, of the last change
    first_change_ms: Option<u64>,
    last_change_ms: u64,
}

impl<G: SettingsGroup, const N: usize> Settings<G, N> {
    /// Every field at its default, nothing loaded yet.
    pub fn new(key: Key, debounce: Debounce) -> Self {
        Settings {
            key,
            value: G::defaults(),
            debounce,
            stored: [0; N],
            stored_len: 0,
            next: [0; N],
            read_only: false,
            force: false,
            seq: 0,
            first_change_ms: None,
            last_change_ms: 0,
        }
    }

    /// Read the group's record from `store`. Every field not in it is at its default. Fails only when the flash
    /// fails; a record this firmware can't read is reported in [LoadReport::unreadable].
    pub async fn load<S: NorFlash, const B: usize>(
        &mut self,
        store: &mut Store<S, B>,
    ) -> Result<LoadReport, Error<S::Error>> {
        self.value = G::defaults();
        self.read_only = false;
        self.force = false;
        self.seq = 0;
        self.first_change_ms = None;
        self.stored_len = 0;
        let mut report = LoadReport::default();
        let fetched = store.fetch(self.key, &mut self.stored).await?;
        self.seq = fetched.max_seq;
        let Some(len) = fetched.len else {
            return Ok(report);
        };
        self.stored_len = len;
        report.found = true;
        match Record::parse(&self.stored[..len]) {
            Ok(rec) => {
                report.writer = Some(rec.writer);
                report.newest = Some(rec.newest);
                match record::decode(&rec, &mut self.value) {
                    Ok(stats) => report.stats = stats,
                    Err(e) => {
                        self.value = G::defaults();
                        self.read_only = true;
                        report.unreadable = Some(RecordError::Malformed(e));
                    }
                }
            }
            Err(e) => {
                self.read_only = true;
                report.unreadable = Some(e);
            }
        }
        Ok(report)
    }

    /// The key of this group in the store.
    pub fn key(&self) -> Key {
        self.key
    }

    /// The current value (what was loaded, with every [set](Self::set) since).
    pub fn get(&self) -> &G {
        &self.value
    }

    /// Change the value in RAM, `now_ms` is any monotonic millisecond clock. Returns whether anything changed; a
    /// change starts the [Debounce].
    pub fn set(&mut self, now_ms: u64, f: impl FnOnce(&mut G)) -> bool {
        let before = self.value.clone();
        f(&mut self.value);
        if self.value == before {
            return false;
        }
        self.last_change_ms = now_ms;
        if self.first_change_ms.is_none() {
            self.first_change_ms = Some(now_ms);
        }
        true
    }

    /// Every field back to its default (a factory reset of this group), also the slots of a newer firmware and a
    /// record that couldn't be read. Written with the next [persist](Self::persist), which the caller does at once.
    pub fn reset_to_defaults(&mut self, now_ms: u64) {
        self.value = G::defaults();
        self.read_only = false;
        self.force = true;
        // forget the stored record: nothing of it is kept, and an empty record is written even if none is stored
        self.stored_len = 0;
        self.last_change_ms = now_ms;
        self.first_change_ms = Some(now_ms);
    }

    /// The stored record can't be read by this firmware (a newer format, or it doesn't decode): it is kept as it
    /// is, [persist](Self::persist) fails with [Error::ReadOnly] until [reset_to_defaults](Self::reset_to_defaults).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// There are changes that aren't stored yet.
    pub fn is_dirty(&self) -> bool {
        self.first_change_ms.is_some()
    }

    /// When [persist_if_due](Self::persist_if_due) will write, `None` with nothing to write.
    pub fn due_at(&self) -> Option<u64> {
        let first = self.first_change_ms?;
        Some(core::cmp::min(
            self.last_change_ms + self.debounce.quiet_ms as u64,
            first + self.debounce.max_ms as u64,
        ))
    }

    /// [persist](Self::persist) when the [Debounce] is over, `Ok(None)` otherwise.
    pub async fn persist_if_due<S: NorFlash, const B: usize>(
        &mut self,
        store: &mut Store<S, B>,
        now_ms: u64,
    ) -> Result<Option<Persisted>, Error<S::Error>> {
        match self.due_at() {
            Some(at) if now_ms >= at => self.persist(store).await.map(Some),
            _ => Ok(None),
        }
    }

    /// Write the record now if it differs from the stored one. Once this returns `Ok`, the value survives a power
    /// cut. On an error the value stays dirty and the stored record is the previous one (or, after a power cut,
    /// either one).
    pub async fn persist<S: NorFlash, const B: usize>(
        &mut self,
        store: &mut Store<S, B>,
    ) -> Result<Persisted, Error<S::Error>> {
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let previous = if self.stored_len > 0 {
            // a stored record that loaded is of FORMAT
            Some(Record::parse(&self.stored[..self.stored_len]).map_err(|_| Error::ReadOnly)?)
        } else {
            None
        };
        // the record as it would be with the stored record's `seq`: the same bytes mean nothing changed. Nothing
        // stored reads as all defaults. A reset writes an empty record over whatever is stored.
        if !self.force {
            let unchanged = match previous.as_ref() {
                Some(prev) => {
                    let (bytes, _) = record::encode(
                        prev.seq,
                        &self.value,
                        Some(prev),
                        &mut self.next,
                        store.scratch(),
                    )
                    .map_err(Error::TooBig)?;
                    *bytes == self.stored[..self.stored_len]
                }
                None => G::SLOTS.iter().all(|s| !self.value.is_set(s.id)),
            };
            if unchanged {
                self.first_change_ms = None;
                return Ok(Persisted::Unchanged);
            }
        }
        let seq = self.seq.wrapping_add(1);
        let (bytes, stats) = record::encode(
            seq,
            &self.value,
            previous.as_ref(),
            &mut self.next,
            store.scratch(),
        )
        .map_err(Error::TooBig)?;
        let len = bytes.len();
        store.store(self.key, &self.next[..len]).await?;
        self.seq = seq;
        core::mem::swap(&mut self.stored, &mut self.next);
        self.stored_len = len;
        self.force = false;
        self.first_change_ms = None;
        Ok(Persisted::Written(stats))
    }

    /// The bytes of the stored record, empty when none is stored.
    pub fn stored_record(&self) -> &[u8] {
        &self.stored[..self.stored_len]
    }
}
