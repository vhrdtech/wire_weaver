//! The record of one settings group, as stored as the value of its key.
//!
//! ```text
//! | format: u8 = 1 | writer: 3 × UNib32 | newest: 3 × UNib32 | pad to a byte | slots: OptionAnyVec |
//! ```
//!
//! - `writer` is the [Version] of the struct's crate that wrote the record, `newest` the highest version whose data
//!   is in it (a downgraded firmware keeps the slots of a newer one, so `newest` stays).
//! - `slots` is an [OptionAnyVec] whose every present slot is a byte blob ([RefVec]`<u8>`): the ShrinkWrap bytes
//!   of that field on their own (`to_ww_bytes`). Since every slot is a blob, any reader can step over a slot it
//!   doesn't know and copy it unchanged, which a plain `sparse` struct can't do (its values carry no length).
//! - A field at its default is absent. No field set is a record of the header only.
//!
//! A reader that sees another `format` doesn't touch the record, see [Settings](struct@crate::Settings).

use crate::group::{SettingsGroup, SlotSet, Version};
use shrink_wrap::option_any_vec::{OptionAnyVec, OptionAnyVecIter, OptionAnyVecWriter, SetBits};
use shrink_wrap::{BufReader, BufWriter, Error, RefVec};

/// The record format this crate reads and writes.
pub const FORMAT: u8 = 1;

/// A record read from flash.
#[derive(Copy, Clone, Debug)]
pub struct Record<'i> {
    /// Version of the crate that wrote this record.
    pub writer: Version,
    /// Highest version whose data is in this record.
    pub newest: Version,
    /// One byte blob per present slot.
    pub slots: OptionAnyVec<'i>,
}

/// Why a record couldn't be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RecordError {
    /// Written in a format this crate doesn't know (a newer one).
    UnknownFormat(u8),
    /// Doesn't decode: written by a buggy writer, the flash store checks a CRC.
    Malformed(Error),
}

impl From<Error> for RecordError {
    fn from(e: Error) -> Self {
        RecordError::Malformed(e)
    }
}

impl<'i> Record<'i> {
    /// Parse the header and the slot vec's header.
    pub fn parse(bytes: &'i [u8]) -> Result<Self, RecordError> {
        let mut rd = BufReader::new(bytes);
        let format = rd.read_u8()?;
        if format != FORMAT {
            return Err(RecordError::UnknownFormat(format));
        }
        let writer = read_version(&mut rd)?;
        let newest = read_version(&mut rd)?;
        rd.align_byte();
        let (pos, _) = rd.pos();
        let slots = OptionAnyVec::new(&bytes[pos..])?;
        Ok(Record {
            writer,
            newest,
            slots,
        })
    }

    /// The present slots with their bytes, in increasing slot order.
    pub fn blobs(&self) -> Blobs<'i> {
        Blobs {
            it: self.slots.iter(),
        }
    }

    /// The bytes of `slot`, `None` when it is absent or the record is malformed.
    pub fn blob(&self, slot: u32) -> Option<&'i [u8]> {
        self.blobs()
            .map_while(Result::ok)
            .find(|(s, _)| *s == slot)
            .map(|(_, b)| b)
    }
}

/// Iterator over the present slots of a [Record]: `(slot, bytes)`.
pub struct Blobs<'i> {
    it: OptionAnyVecIter<'i>,
}

impl<'i> Iterator for Blobs<'i> {
    type Item = Result<(u32, &'i [u8]), Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let slot = self.it.next_present()?;
        match self.it.next::<RefVec<'i, u8>>() {
            Ok(Some(blob)) => Some(Ok((slot, blob.as_slice_long()))),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

trait AsSliceLong<'i> {
    fn as_slice_long(&self) -> &'i [u8];
}

impl<'i> AsSliceLong<'i> for RefVec<'i, u8> {
    fn as_slice_long(&self) -> &'i [u8] {
        match self {
            RefVec::Slice { slice } => slice,
            RefVec::Buf {
                buf,
                elements_count,
            } => {
                let mut buf = *buf;
                buf.read_raw_slice(*elements_count).unwrap_or(&[])
            }
        }
    }
}

fn read_version(rd: &mut BufReader) -> Result<Version, Error> {
    Ok(Version::new(
        rd.read_unib32()?,
        rd.read_unib32()?,
        rd.read_unib32()?,
    ))
}

fn write_version(wr: &mut BufWriter, v: Version) -> Result<(), Error> {
    wr.write_unib32(v.major)?;
    wr.write_unib32(v.minor)?;
    wr.write_unib32(v.patch)
}

/// What [encode] did with the slots of the previous record.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct EncodeStats {
    /// Fields written, set to something other than their default.
    pub fields: u16,
    /// Slots this firmware doesn't know, copied unchanged.
    pub unknown_kept: u16,
    /// Fields at their default whose stored bytes this firmware can't decode (written by a newer one), copied
    /// unchanged.
    pub undecodable_kept: u16,
    /// Slots of removed fields dropped.
    pub retired_dropped: u16,
}

/// Write the record of `value` into `out`, keeping what `previous` holds that this firmware doesn't know.
///
/// - A field at its default is absent.
/// - A field whose bytes in `previous` decode to its current value is copied byte for byte, so the fields a newer
///   firmware appended inside that field's type survive.
/// - A slot this firmware doesn't know is copied unchanged, a retired one is dropped.
/// - A field at its default whose bytes in `previous` don't decode (a newer firmware wrote a value this one doesn't
///   know, a new enum variant say) is copied unchanged, until the field is set to something else.
///
/// `scratch` holds the bytes of one field at a time. `previous` must be a record of [FORMAT] (or `None`).
pub fn encode<'o, G: SettingsGroup>(
    value: &G,
    previous: Option<&Record<'_>>,
    out: &'o mut [u8],
    scratch: &mut [u8],
) -> Result<(&'o [u8], EncodeStats), Error> {
    let mut stats = EncodeStats::default();
    let mut present = SlotSet::default();
    for s in G::SLOTS {
        if value.is_set(s.id) {
            present.insert(s.id);
            stats.fields += 1;
        }
    }
    let newest = match previous {
        Some(prev) => {
            for blob in prev.blobs() {
                let (slot, b) = blob?;
                if G::is_retired(slot) {
                    stats.retired_dropped += 1;
                } else if G::is_known(slot) {
                    if !value.is_set(slot) && !G::slot_decodes(slot, b) {
                        present.insert(slot);
                        stats.undecodable_kept += 1;
                    }
                } else {
                    if !present.insert(slot) {
                        // above MAX_SLOT, can't come from a derive: drop it
                        continue;
                    }
                    stats.unknown_kept += 1;
                }
            }
            core::cmp::max(prev.newest, G::VERSION)
        }
        None => G::VERSION,
    };

    let mut wr = BufWriter::new(out);
    wr.write_u8(FORMAT)?;
    write_version(&mut wr, G::VERSION)?;
    write_version(&mut wr, newest)?;
    let header_len = wr.finish_and_take()?.len();

    let rest = &mut out[header_len..];
    let mut vec = OptionAnyVecWriter::new(rest, SetBits::new(present.words()))?;
    let mut prev_blobs = previous.map(|p| p.blobs());
    let mut prev_next: Option<(u32, &[u8])> = None;
    while let Some(slot) = vec.next_slot() {
        // advance the previous record's blobs up to `slot`
        let mut prev_blob = None;
        if let Some(blobs) = prev_blobs.as_mut() {
            loop {
                if prev_next.is_none() {
                    prev_next = blobs.next().transpose()?;
                }
                match prev_next {
                    // a retired slot or one above MAX_SLOT, not written
                    Some((s, _)) if s < slot => prev_next = None,
                    Some((s, b)) if s == slot => {
                        prev_blob = Some(b);
                        prev_next = None;
                        break;
                    }
                    _ => break,
                }
            }
        }
        if G::is_known(slot) {
            match prev_blob {
                Some(b)
                    if value.slot_eq(slot, b)
                        || (!value.is_set(slot) && !G::slot_decodes(slot, b)) =>
                {
                    vec.write(slot, &RefVec::new_bytes(b))?
                }
                _ => {
                    let bytes = value.write_slot(slot, scratch)?;
                    vec.write(slot, &RefVec::new_bytes(bytes))?;
                }
            }
        } else {
            // only unknown slots of the previous record are in the present set besides the known ones
            let b = prev_blob.ok_or(Error::OptionAnyVecWrongSlot)?;
            vec.write(slot, &RefVec::new_bytes(b))?;
        }
    }
    let vec_len = vec.finish_and_take()?.len();
    Ok((&out[..header_len + vec_len], stats))
}

/// What [decode] found in a record.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DecodeStats {
    /// Fields read from the record.
    pub fields: u16,
    /// Fields whose bytes didn't decode (a newer firmware's value): left at their default, the bytes kept on the
    /// next write unless the field is set.
    pub bad_fields: u16,
    /// Slots of a newer firmware, kept on the next write.
    pub unknown: u16,
    /// Slots of removed fields, dropped on the next write.
    pub retired: u16,
}

/// Read `record` into `value` (which should be at its defaults): every known slot that decodes is set.
pub fn decode<G: SettingsGroup>(record: &Record<'_>, value: &mut G) -> Result<DecodeStats, Error> {
    let mut stats = DecodeStats::default();
    for blob in record.blobs() {
        let (slot, bytes) = blob?;
        if G::is_known(slot) {
            match value.read_slot(slot, bytes) {
                Ok(()) => stats.fields += 1,
                Err(_) => {
                    value.reset_slot(slot);
                    stats.bad_fields += 1;
                }
            }
        } else if G::is_retired(slot) {
            stats.retired += 1;
        } else {
            stats.unknown += 1;
        }
    }
    Ok(stats)
}
