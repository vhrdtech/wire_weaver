//! [OptionAnyVec]: a zero-copy, no_std and no-alloc array of heterogeneous `Option<Any>` slots (SW-55).
//!
//! Layout, see `docs/serdes/shrink_wrap.md` for the details and byte-verified examples:
//!
//! `|presence header|values of the present slots, in slot order, no padding between them|reverse UNib32 lengths|`
//!
//! The header is one mode nibble followed by what the mode needs, and the writer picks the smallest mode for the
//! set of present slots it is given up front:
//!
//! | Mode nibble | Name       | After the mode nibble                                                         |
//! |-------------|------------|-------------------------------------------------------------------------------|
//! | `0`         | all        | `n: UNib32`: slots `0..n` present                                              |
//! | `1`         | dense      | `len: UNib32`, then `len` bits, one per slot `0..len`                          |
//! | `2`         | two-level  | `groups: UNib32`, a bit per group of 8 slots, then 8 bits per non-empty group  |
//! | `3`         | index list | `count: UNib32`, then `count` UNib32: the first slot, then `slot - prev - 1`   |
//! | `4..=15`    | reserved   | a reader fails with [Error::EnumFutureVersionOrMalformedData]                  |
//!
//! No present slot at all is no bytes. The header describes the writer's present set on its own, without the
//! slot count of either side: a reader that knows fewer slots stops after its last one (slots are only ever
//! appended, so the values it doesn't know come last), a reader that knows more sees the extra slots absent.
#[cfg(feature = "std")]
use crate::DeserializeShrinkWrapOwned;
use crate::buf_writer::UnsizedBuilder;
use crate::traits::ElementSize;
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, Error, SerializeShrinkWrap};
use core::fmt::{Debug, Formatter};

/// Zero-copy, no_std and no-alloc array of `Option<Any>` slots.
///
/// Each slot is absent or holds a value of any type that implements [SerializeShrinkWrap] /
/// [DeserializeShrinkWrap] (numbers, structs, `Vec`, `String`, enums, a nested `OptionAnyVec`), slots don't have
/// to be of the same type. Only the present values take space, packed in slot order after a presence header (see
/// the [module docs](self)). Like [EitherAnyVec](crate::either_any_vec::EitherAnyVec) this is low-level machinery:
/// the type of each slot is known to both sides, not stored.
///
/// Write with [OptionAnyVecWriter] (own buffer) or [OptionAnyVecBuilder] (in place, into a parent's
/// [BufWriter]), read with [iter](Self::iter). The header is checked when the vec is read
/// ([new](Self::new) or as a field), a malformed one is an [Error].
///
/// The type is `Unsized`: as a field of a struct, or a slot of another `OptionAnyVec`, its size is stored.
#[derive(Clone, Copy)]
pub struct OptionAnyVec<'i> {
    data: &'i [u8],
    header: Header<'i>,
    values: BufReader<'i>,
}

/// How the presence header lists the present slots, see the [module docs](self).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PresenceMode {
    /// Slots `0..n` are present: one `UNib32`.
    All = 0,
    /// One bit per slot up to the last present one.
    Dense = 1,
    /// One bit per group of 8 slots up to the last present one, then 8 bits per non-empty group.
    TwoLevel = 2,
    /// The present slots as a list of `UNib32` deltas.
    IndexList = 3,
}

/// The present slot ids of an [OptionAnyVec], in order, from its header only.
#[derive(Clone)]
pub struct PresentSlots<'i> {
    header: Header<'i>,
    /// Dense, All: next slot to look at; TwoLevel: next group to look at; IndexList: next slot id at least
    next: u32,
    /// TwoLevel: the bits of the current group not returned yet
    group_byte: u8,
    /// TwoLevel: the first slot of the current group
    group_base: u32,
}

/// Reads the slots of an [OptionAnyVec] one after another, see [OptionAnyVec::iter].
pub struct OptionAnyVecIter<'i> {
    present: PresentSlots<'i>,
    upcoming: Option<u32>,
    slot: u32,
    rd: BufReader<'i>,
}

#[derive(Copy, Clone)]
enum Header<'i> {
    Empty,
    All {
        n: u32,
    },
    Dense {
        len: u32,
        bits: BufReader<'i>,
    },
    TwoLevel {
        groups: u32,
        group_bits: BufReader<'i>,
        group_bytes: BufReader<'i>,
    },
    IndexList {
        count: u32,
        deltas: BufReader<'i>,
    },
}

const MODE_ALL: u8 = PresenceMode::All as u8;
const MODE_DENSE: u8 = PresenceMode::Dense as u8;
const MODE_TWO_LEVEL: u8 = PresenceMode::TwoLevel as u8;
const MODE_INDEX_LIST: u8 = PresenceMode::IndexList as u8;

impl<'i> OptionAnyVec<'i> {
    /// Parse the header of a vec that takes all of `data` (as written by [OptionAnyVecWriter::finish_and_take]).
    pub fn new(data: &'i [u8]) -> Result<Self, Error> {
        let mut rd = BufReader::new(data);
        let header = Header::read(&mut rd)?;
        Ok(OptionAnyVec {
            data,
            header,
            values: rd,
        })
    }

    /// The bytes of the vec, header first.
    pub fn as_bytes(&self) -> &'i [u8] {
        self.data
    }

    /// No slot is present.
    pub fn is_empty(&self) -> bool {
        matches!(self.header, Header::Empty)
    }

    /// The mode of the header, `None` when no slot is present.
    pub fn mode(&self) -> Option<PresenceMode> {
        match self.header {
            Header::Empty => None,
            Header::All { .. } => Some(PresenceMode::All),
            Header::Dense { .. } => Some(PresenceMode::Dense),
            Header::TwoLevel { .. } => Some(PresenceMode::TwoLevel),
            Header::IndexList { .. } => Some(PresenceMode::IndexList),
        }
    }

    /// The ids of the present slots, in order, without reading any value.
    ///
    /// The ids come from untrusted data and are not bounded by any slot count: stop at the last slot you know.
    pub fn present(&self) -> PresentSlots<'i> {
        PresentSlots {
            header: self.header,
            next: 0,
            group_byte: 0,
            group_base: 0,
        }
    }

    /// Read the slots in order, starting at slot 0.
    pub fn iter(&self) -> OptionAnyVecIter<'i> {
        let mut present = self.present();
        let upcoming = present.next();
        OptionAnyVecIter {
            present,
            upcoming,
            slot: 0,
            rd: self.values,
        }
    }
}

impl Debug for OptionAnyVec<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "OptionAnyVec({:02x?})", self.data)
    }
}

impl PartialEq for OptionAnyVec<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for OptionAnyVec<'_> {}

#[cfg(feature = "defmt")]
impl defmt::Format for OptionAnyVec<'_> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "OptionAnyVec({=[u8]:x})", self.data)
    }
}

impl<'i> Header<'i> {
    /// Read and check the header, `rd` is left at the first value. Every count is checked against the data left
    /// before anything is looped over, so a malformed header fails in time bounded by its length.
    fn read(rd: &mut BufReader<'i>) -> Result<Self, Error> {
        if rd.bytes_left() == 0 {
            return Ok(Header::Empty);
        }
        let mode = rd.read_nib_value()?;
        match mode {
            MODE_ALL => {
                let n = rd.read_unib32()?;
                if n == 0 {
                    Ok(Header::Empty)
                } else {
                    Ok(Header::All { n })
                }
            }
            MODE_DENSE => {
                let len = rd.read_unib32()?;
                let bits = *rd;
                skip_bits(rd, len)?;
                Ok(Header::Dense { len, bits })
            }
            MODE_TWO_LEVEL => {
                let groups = rd.read_unib32()?;
                if groups > MAX_GROUPS {
                    return Err(Error::MalformedOptionAnyVec);
                }
                let group_bits = *rd;
                if (groups as usize) > rd.bits_left() {
                    return Err(Error::MalformedOptionAnyVec);
                }
                let mut non_empty = 0;
                for _ in 0..groups {
                    non_empty += rd.read_bool()? as u32;
                }
                let group_bytes = *rd;
                let mut check = *rd;
                skip_bits(rd, non_empty * 8)?;
                for _ in 0..non_empty {
                    // the writer never lists an empty group
                    if check.read_un8(8)? == 0 {
                        return Err(Error::MalformedOptionAnyVec);
                    }
                }
                Ok(Header::TwoLevel {
                    groups,
                    group_bits,
                    group_bytes,
                })
            }
            MODE_INDEX_LIST => {
                let count = rd.read_unib32()?;
                // every delta takes one nibble at least
                if count as usize > rd.nibbles_left() {
                    return Err(Error::MalformedOptionAnyVec);
                }
                let deltas = *rd;
                let mut next = 0u32;
                for _ in 0..count {
                    let delta = rd.read_unib32()?;
                    // ids stay below u32::MAX, as the writer's
                    next = next
                        .checked_add(delta)
                        .and_then(|id| id.checked_add(1))
                        .ok_or(Error::MalformedOptionAnyVec)?;
                }
                if count == 0 {
                    Ok(Header::Empty)
                } else {
                    Ok(Header::IndexList { count, deltas })
                }
            }
            _ => Err(Error::EnumFutureVersionOrMalformedData),
        }
    }
}

/// Slot ids are below `u32::MAX`, so there are at most this many groups of 8
const MAX_GROUPS: u32 = u32::MAX / 8 + 1;

fn skip_bits(rd: &mut BufReader<'_>, bits: u32) -> Result<(), Error> {
    if bits as usize > rd.bits_left() {
        return Err(Error::MalformedOptionAnyVec);
    }
    let mut left = bits;
    while left > 0 {
        let n = left.min(32) as u8;
        rd.read_un32(n)?;
        left -= n as u32;
    }
    Ok(())
}

impl Iterator for PresentSlots<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        // The header was checked when the vec was read: these reads stay within it and don't fail.
        match &mut self.header {
            Header::Empty => None,
            Header::All { n } => {
                if self.next < *n {
                    self.next += 1;
                    Some(self.next - 1)
                } else {
                    None
                }
            }
            Header::Dense { len, bits } => {
                while self.next < *len {
                    let slot = self.next;
                    self.next += 1;
                    if bits.read_bool().ok()? {
                        return Some(slot);
                    }
                }
                None
            }
            Header::TwoLevel {
                groups,
                group_bits,
                group_bytes,
            } => loop {
                if self.group_byte != 0 {
                    let bit = self.group_byte.leading_zeros();
                    self.group_byte &= !(0x80 >> bit);
                    return Some(self.group_base + bit);
                }
                if self.next >= *groups {
                    return None;
                }
                if group_bits.read_bool().ok()? {
                    self.group_byte = group_bytes.read_un8(8).ok()?;
                    self.group_base = self.next * 8;
                }
                self.next += 1;
            },
            Header::IndexList { count, deltas } => {
                if *count == 0 {
                    return None;
                }
                *count -= 1;
                let slot = self.next + deltas.read_unib32().ok()?;
                self.next = slot + 1;
                Some(slot)
            }
        }
    }
}

impl<'i> OptionAnyVecIter<'i> {
    /// The slot the next call to [next](Self::next) reads.
    pub fn slot(&self) -> u32 {
        self.slot
    }

    /// Read the next slot: `None` when it is absent, also for every slot past the writer's last one.
    // Not an Iterator: the item type is chosen per call.
    #[allow(clippy::should_implement_trait)]
    pub fn next<T: DeserializeShrinkWrap<'i>>(&mut self) -> Result<Option<T>, Error> {
        if self.advance() {
            Ok(Some(self.rd.read()?))
        } else {
            Ok(None)
        }
    }

    /// [next](Self::next) for [DeserializeShrinkWrapOwned] types.
    #[cfg(feature = "std")]
    pub fn next_owned<T: DeserializeShrinkWrapOwned>(&mut self) -> Result<Option<T>, Error> {
        if self.advance() {
            Ok(Some(self.rd.read_owned()?))
        } else {
            Ok(None)
        }
    }

    /// Skip the absent slots up to the next present one and return its id, the next call to [next](Self::next)
    /// reads its value. `None` when there is no present slot left.
    pub fn next_present(&mut self) -> Option<u32> {
        let slot = self.upcoming?;
        self.slot = slot;
        Some(slot)
    }

    /// Whether the current slot is present, moves on to the next one
    fn advance(&mut self) -> bool {
        let present = self.upcoming == Some(self.slot);
        if present {
            self.upcoming = self.present.next();
        }
        self.slot = self.slot.saturating_add(1);
        present
    }
}

impl SerializeShrinkWrap for OptionAnyVec<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_raw_slice(self.data)
    }
}

#[cfg(feature = "std")]
impl crate::SerializeShrinkWrapOwned for OptionAnyVec<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        wr.write_raw_slice(self.data)
    }
}

impl<'i> DeserializeShrinkWrap<'i> for OptionAnyVec<'i> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        OptionAnyVec::new(rd.read_raw_slice(rd.bytes_left())?)
    }
}

/// no_std and no-alloc writer of an [OptionAnyVec] in place, into a [BufWriter] that may hold other data too.
///
/// The present slots are given up front, in increasing order (an iterator of slot ids, e.g. [SetBits] over a
/// bitset): the header is written first, then each present slot's value in order, with [write](Self::write) or
/// [start_slot](Self::start_slot) and a write of your own. [finish](Self::finish) checks that every present slot
/// got its value.
pub struct OptionAnyVecBuilder<I> {
    pending: I,
    size: Option<UnsizedBuilder>,
}

impl<I: Iterator<Item = u32> + Clone> OptionAnyVecBuilder<I> {
    /// Write the header for the `present` slots with the smallest mode, at the current position of `wr`.
    ///
    /// For a vec that is the root of `wr` (all of the data written). For a field of a struct or a slot of another
    /// `OptionAnyVec`, use [new_nested](Self::new_nested).
    pub fn new<P: IntoIterator<IntoIter = I>>(
        wr: &mut BufWriter<'_>,
        present: P,
    ) -> Result<Self, Error> {
        Self::start(wr, present.into_iter(), None, None)
    }

    /// [new](Self::new) with the given header mode instead of the smallest one, [PresenceMode::All] only works
    /// when the slots `0..n` are present ([Error::OptionAnyVecWrongSlot] otherwise).
    pub fn new_with_mode<P: IntoIterator<IntoIter = I>>(
        wr: &mut BufWriter<'_>,
        present: P,
        mode: PresenceMode,
    ) -> Result<Self, Error> {
        Self::start(wr, present.into_iter(), Some(mode), None)
    }

    /// [new](Self::new) for a vec that is a field of a struct or the value of a slot of another `OptionAnyVec`:
    /// its size is stored as for every `Unsized` value (what [BufWriter::write] does), [finish](Self::finish)
    /// fills it in.
    pub fn new_nested<P: IntoIterator<IntoIter = I>>(
        wr: &mut BufWriter<'_>,
        present: P,
    ) -> Result<Self, Error> {
        let size = UnsizedBuilder::new(wr)?;
        Self::start(wr, present.into_iter(), None, Some(size))
    }

    fn start(
        wr: &mut BufWriter<'_>,
        present: I,
        mode: Option<PresenceMode>,
        size: Option<UnsizedBuilder>,
    ) -> Result<Self, Error> {
        let stats = SlotStats::scan(present.clone())?;
        if stats.count > 0 {
            let mode = match mode {
                Some(PresenceMode::All) if !stats.is_all() => {
                    return Err(Error::OptionAnyVecWrongSlot);
                }
                Some(mode) => mode,
                None => stats.smallest_mode(),
            };
            write_header(wr, present.clone(), &stats, mode)?;
        }
        Ok(OptionAnyVecBuilder {
            pending: present,
            size,
        })
    }

    /// The next slot that needs a value, `None` when all are written.
    pub fn next_slot(&self) -> Option<u32> {
        self.pending.clone().next()
    }

    /// Write the value of present `slot`, which must be the [next one](Self::next_slot)
    /// ([Error::OptionAnyVecWrongSlot] otherwise).
    pub fn write<T: SerializeShrinkWrap>(
        &mut self,
        slot: u32,
        value: &T,
        wr: &mut BufWriter<'_>,
    ) -> Result<(), Error> {
        self.start_slot(slot)?;
        wr.write(value)
    }

    /// Mark present `slot` as written, for a value written into `wr` by other means. It must be written as
    /// [BufWriter::write] would write it: an `Unsized` value with its size, e.g. a nested vec through
    /// [new_nested](Self::new_nested).
    pub fn start_slot(&mut self, slot: u32) -> Result<(), Error> {
        if self.pending.next() == Some(slot) {
            Ok(())
        } else {
            Err(Error::OptionAnyVecWrongSlot)
        }
    }

    /// Check that every present slot got its value, and fill in the size of a [nested](Self::new_nested) vec.
    pub fn finish(mut self, wr: &mut BufWriter<'_>) -> Result<(), Error> {
        if self.pending.next().is_some() {
            return Err(Error::OptionAnyVecWrongSlot);
        }
        if let Some(size) = self.size {
            size.finish(wr)?;
        }
        Ok(())
    }
}

/// no_std and no-alloc writer of an [OptionAnyVec] into a buffer of its own, see [OptionAnyVecBuilder].
pub struct OptionAnyVecWriter<'i, I> {
    wr: BufWriter<'i>,
    builder: OptionAnyVecBuilder<I>,
}

impl<'i, I: Iterator<Item = u32> + Clone> OptionAnyVecWriter<'i, I> {
    /// Write the header for the `present` slots (increasing ids) into `buf`.
    pub fn new<P: IntoIterator<IntoIter = I>>(
        buf: &'i mut [u8],
        present: P,
    ) -> Result<Self, Error> {
        let mut wr = BufWriter::new(buf);
        let builder = OptionAnyVecBuilder::new(&mut wr, present)?;
        Ok(Self { wr, builder })
    }

    /// [new](Self::new) with the given header mode, see [OptionAnyVecBuilder::new_with_mode].
    pub fn new_with_mode<P: IntoIterator<IntoIter = I>>(
        buf: &'i mut [u8],
        present: P,
        mode: PresenceMode,
    ) -> Result<Self, Error> {
        let mut wr = BufWriter::new(buf);
        let builder = OptionAnyVecBuilder::new_with_mode(&mut wr, present, mode)?;
        Ok(Self { wr, builder })
    }

    /// The next slot that needs a value, `None` when all are written.
    pub fn next_slot(&self) -> Option<u32> {
        self.builder.next_slot()
    }

    /// Write the value of present `slot`, which must be the [next one](Self::next_slot).
    pub fn write<T: SerializeShrinkWrap>(&mut self, slot: u32, value: &T) -> Result<(), Error> {
        self.builder.write(slot, value, &mut self.wr)
    }

    /// Check that every present slot got its value and get the bytes.
    pub fn finish_and_take(mut self) -> Result<&'i [u8], Error> {
        self.builder.finish(&mut self.wr)?;
        self.wr.finish_and_take()
    }
}

/// Increasing slot ids of the set bits of a bitset, bit 0 of `words[0]` is slot 0: a present set for
/// [OptionAnyVecBuilder].
#[derive(Clone)]
pub struct SetBits<'a> {
    words: &'a [u32],
    word_idx: u32,
    word: u32,
}

impl<'a> SetBits<'a> {
    pub fn new(words: &'a [u32]) -> Self {
        SetBits {
            words,
            word_idx: 0,
            word: words.first().copied().unwrap_or(0),
        }
    }
}

impl Iterator for SetBits<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        while self.word == 0 {
            self.word_idx += 1;
            self.word = *self.words.get(self.word_idx as usize)?;
        }
        let bit = self.word.trailing_zeros();
        self.word &= self.word - 1;
        Some(self.word_idx * 32 + bit)
    }
}

/// What the writer needs to know about a present set to pick a mode
#[derive(Default)]
struct SlotStats {
    count: u32,
    /// Last slot id + 1
    len: u32,
    index_list_nibbles: u32,
    non_empty_groups: u32,
}

impl SlotStats {
    fn scan(present: impl Iterator<Item = u32>) -> Result<Self, Error> {
        let mut stats = SlotStats::default();
        let mut last_group = None;
        for slot in present {
            // ids increase and stay below u32::MAX, so that `len` fits
            if slot < stats.len || slot == u32::MAX {
                return Err(Error::OptionAnyVecWrongSlot);
            }
            stats.index_list_nibbles += unib32_nibbles(slot - stats.len);
            stats.len = slot + 1;
            stats.count += 1;
            if last_group != Some(slot / 8) {
                last_group = Some(slot / 8);
                stats.non_empty_groups += 1;
            }
        }
        Ok(stats)
    }

    fn is_all(&self) -> bool {
        self.count == self.len
    }

    /// Header bits of a mode, from a nibble boundary
    fn header_bits(&self, mode: PresenceMode) -> u64 {
        let groups = self.len.div_ceil(8);
        4 + match mode {
            PresenceMode::All => 4 * unib32_nibbles(self.count) as u64,
            PresenceMode::Dense => 4 * unib32_nibbles(self.len) as u64 + self.len as u64,
            PresenceMode::TwoLevel => {
                4 * unib32_nibbles(groups) as u64 + groups as u64 + 8 * self.non_empty_groups as u64
            }
            PresenceMode::IndexList => {
                4 * (unib32_nibbles(self.count) + self.index_list_nibbles) as u64
            }
        }
    }

    /// The mode with the fewest header bits, the first one of All, Dense, TwoLevel, IndexList on a tie
    fn smallest_mode(&self) -> PresenceMode {
        let mut best = PresenceMode::Dense;
        if self.is_all() {
            best = PresenceMode::All;
        }
        for mode in [PresenceMode::TwoLevel, PresenceMode::IndexList] {
            if self.header_bits(mode) < self.header_bits(best) {
                best = mode;
            }
        }
        best
    }
}

fn unib32_nibbles(val: u32) -> u32 {
    crate::nib32::UNib32(val).len_nibbles() as u32
}

fn write_header(
    wr: &mut BufWriter<'_>,
    present: impl Iterator<Item = u32> + Clone,
    stats: &SlotStats,
    mode: PresenceMode,
) -> Result<(), Error> {
    wr.write_nib_masked(mode as u8)?;
    match mode {
        PresenceMode::All => wr.write_unib32(stats.count)?,
        PresenceMode::Dense => {
            wr.write_unib32(stats.len)?;
            let mut present = present.peekable();
            for slot in 0..stats.len {
                let is_present = present.next_if_eq(&slot).is_some();
                wr.write_bool(is_present)?;
            }
        }
        PresenceMode::TwoLevel => {
            let groups = stats.len.div_ceil(8);
            wr.write_unib32(groups)?;
            let mut slots = present.clone().peekable();
            for group in 0..groups {
                let mut non_empty = false;
                while slots.next_if(|slot| slot / 8 == group).is_some() {
                    non_empty = true;
                }
                wr.write_bool(non_empty)?;
            }
            let mut slots = present.peekable();
            while let Some(first) = slots.peek().copied() {
                let group = first / 8;
                let mut byte = 0u8;
                while let Some(slot) = slots.next_if(|slot| slot / 8 == group) {
                    byte |= 0x80 >> (slot % 8);
                }
                wr.write_un8(8, byte)?;
            }
        }
        PresenceMode::IndexList => {
            wr.write_unib32(stats.count)?;
            let mut next = 0;
            for slot in present {
                wr.write_unib32(slot - next)?;
                next = slot + 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
