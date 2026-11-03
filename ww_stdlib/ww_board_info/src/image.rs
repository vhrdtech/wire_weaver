//! The reworks an image knows, inside the image (STD-101).
//!
//! A flasher must know which reworks a new image knows before it writes it, without a round trip to that image. So
//! every image that guards ([crate::guard]) carries a small record in flash: per board and revision, the reworks it
//! knows. [image_reworks!](crate::image_reworks) builds it at compile time from the same [ReworkTable] consts the
//! firmware uses; a flasher finds it in the bytes it is about to write with [ImageTable::find] and compares with
//! [check].
//!
//! The firmware reads its known reworks back through the record ([ImageReworks::known]) when it decides at boot.
//! That keeps the record in the image (the linker can't drop what the boot code reads) and makes it the very table
//! the guard used.
//!
//! ```
//! use ww_board_info::{ReworkId, image::{ImageTable, check, Check}, table::{ReworkDef, ReworkSet, ReworkTable}};
//!
//! const B200A: ReworkTable = ReworkTable::new(&[ReworkDef::new(1, "analog buffer supply"), ReworkDef::new(5, "SD full swap")], &[]);
//! const NONE: ReworkTable = ReworkTable::new(&[], &[]);
//! ww_board_info::image_reworks!(static IMAGE_REWORKS = [("B200", "A", B200A), ("B200", "B", NONE)]);
//!
//! // firmware, at boot
//! assert_eq!(IMAGE_REWORKS.known("B200", "A"), B200A.known());
//!
//! // a flasher, on the bytes it is about to write
//! let mut flash = vec![0xff; 1000];
//! flash.extend_from_slice(IMAGE_REWORKS.as_bytes());
//! let table = ImageTable::find(&flash).unwrap().unwrap();
//! let applied = ReworkSet::EMPTY.with(ReworkId(5), true).with(ReworkId(9), true);
//! assert_eq!(check(Some(&table), "B200", "A", applied), Check::Lacking(ReworkSet::EMPTY.with(ReworkId(9), true)));
//! ```
//!
//! # Layout
//!
//! Little-endian, no padding, `16 + 48 × count` bytes, anywhere in flash (alignment 1):
//!
//! | Offset | Size | Field                                                                     |
//! |--------|------|---------------------------------------------------------------------------|
//! | 0      | 8    | magic `WWREWORK`                                                          |
//! | 8      | 1    | format version, 1                                                         |
//! | 9      | 1    | `count`, entries that follow                                              |
//! | 10     | 2    | entry size, 48                                                            |
//! | 12     | 4    | CRC-32 (IEEE, as zlib) of bytes 8..12 and every entry                     |
//! | 16     | 48×n | entries: board name (32, zero-padded UTF-8), revision (8, zero-padded), known reworks (`u64`, bit n = rework n) |
//!
//! A flasher scans the image's flash contents for the magic, so ELF, UF2 and raw binaries are read the same way
//! once loaded; scan the assembled contents, not a UF2 file (its 256-byte payloads can split the record). A version
//! above 1 may append fields to the header or the entries (entry size says how long they are): readers skip what
//! they don't know.

use core::fmt;

use crate::table::{ReworkSet, ReworkTable};

/// The record's first bytes.
pub const MAGIC: [u8; 8] = *b"WWREWORK";
/// Format this crate writes.
pub const VERSION: u8 = 1;
/// Header bytes before the entries.
pub const HEADER_LEN: usize = 16;
/// Bytes per entry this crate writes.
pub const ENTRY_LEN: usize = 48;
/// Longest board name in an entry ([crate::BoardSummary::name] holds 32).
pub const NAME_LEN: usize = 32;
/// Longest revision in an entry ([crate::BoardSummary::revision] holds 8).
pub const REVISION_LEN: usize = 8;

/// Bytes of a record with `count` entries.
pub const fn record_len(count: usize) -> usize {
    HEADER_LEN + count * ENTRY_LEN
}

/// One board revision an image knows, for [image_reworks!](crate::image_reworks).
#[derive(Clone, Copy, Debug)]
pub struct ImageBoard<'a> {
    pub name: &'a str,
    pub revision: &'a str,
    pub table: ReworkTable<'a>,
}

/// The record of an image, built at compile time; see the [module](self) docs. `N` is
/// [record_len]`(count)`.
#[repr(transparent)]
pub struct ImageReworks<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> ImageReworks<N> {
    /// Panics (at compile time in a `static`) when `N` doesn't fit the boards, there are more than 255, a name or
    /// revision is too long, or a board revision is listed twice.
    pub const fn new(boards: &[ImageBoard]) -> Self {
        assert!(boards.len() <= u8::MAX as usize, "more than 255 boards");
        assert!(N == record_len(boards.len()), "record length doesn't match");
        let mut bytes = [0u8; N];
        copy(&mut bytes, 0, &MAGIC);
        bytes[8] = VERSION;
        bytes[9] = boards.len() as u8;
        copy(&mut bytes, 10, &(ENTRY_LEN as u16).to_le_bytes());
        let mut i = 0;
        while i < boards.len() {
            let b = &boards[i];
            assert!(
                !b.name.is_empty() && b.name.len() <= NAME_LEN,
                "board name empty or longer than 32 bytes"
            );
            assert!(
                b.revision.len() <= REVISION_LEN,
                "revision longer than 8 bytes"
            );
            let mut j = 0;
            while j < i {
                assert!(
                    !(eq(boards[j].name, b.name) && eq(boards[j].revision, b.revision)),
                    "board revision listed twice"
                );
                j += 1;
            }
            let at = HEADER_LEN + i * ENTRY_LEN;
            copy(&mut bytes, at, b.name.as_bytes());
            copy(&mut bytes, at + NAME_LEN, b.revision.as_bytes());
            let mut known = 0u64;
            let mut r = 0;
            while r < b.table.reworks.len() {
                // ReworkDef::new keeps numbers below 64
                known |= 1 << b.table.reworks[r].id.0;
                r += 1;
            }
            copy(
                &mut bytes,
                at + NAME_LEN + REVISION_LEN,
                &known.to_le_bytes(),
            );
            i += 1;
        }
        let crc = crc32(&bytes, 8, N);
        copy(&mut bytes, 12, &crc.to_le_bytes());
        ImageReworks { bytes }
    }

    /// The record as it is in flash.
    pub fn as_bytes(&'static self) -> &'static [u8] {
        // through black_box: the boot code reads the record itself, so the linker keeps it in the image
        core::hint::black_box(&self.bytes)
    }

    /// The reworks this image knows for a board revision, read from the record; empty for one it doesn't list (then
    /// every applied rework is unknown). What a firmware passes to [crate::guard::decide].
    pub fn known(&'static self, name: &str, revision: &str) -> ReworkSet {
        ImageTable::parse(self.as_bytes())
            .ok()
            .and_then(|t| t.known(name, revision))
            .unwrap_or(ReworkSet::EMPTY)
    }
}

/// Build the image's rework record as a `static`, see the [module](self) docs:
/// `image_reworks!(pub static IMAGE_REWORKS = [("B200", "A", REWORKS_B200A), ("B200", "B", NO_REWORKS)]);`
/// (board name and revision as `BOARD` reports them, a [ReworkTable]).
#[macro_export]
macro_rules! image_reworks {
    ($vis:vis static $name:ident = [$(($board:expr, $revision:expr, $table:expr)),* $(,)?]) => {
        #[used]
        $vis static $name: $crate::image::ImageReworks<
            { $crate::image::record_len(<[()]>::len(&[$($crate::image_reworks!(@one $board)),*])) },
        > = $crate::image::ImageReworks::new(&[$($crate::image::ImageBoard {
            name: $board,
            revision: $revision,
            table: $table,
        }),*]);
    };
    (@one $x:expr) => { () };
}

/// A record found in image bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageTable<'a> {
    bytes: &'a [u8],
    count: usize,
    entry_len: usize,
}

/// Why a record isn't usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageTableError {
    /// The magic is there, the rest doesn't fit or its CRC is wrong.
    Corrupt,
    /// A format this reader doesn't know (version 0, or entries shorter than version 1's).
    Version(u8),
    /// Two valid records: can't tell which one the image runs on.
    Ambiguous,
}

impl fmt::Display for ImageTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageTableError::Corrupt => f.write_str("the image's rework record is corrupt"),
            ImageTableError::Version(v) => write!(f, "the image's rework record has format {v}"),
            ImageTableError::Ambiguous => f.write_str("the image has two rework records"),
        }
    }
}

impl core::error::Error for ImageTableError {}

impl<'a> ImageTable<'a> {
    /// Parse a record at the start of `bytes` (more bytes may follow).
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ImageTableError> {
        if bytes.get(..8) != Some(&MAGIC) {
            return Err(ImageTableError::Corrupt);
        }
        let header = bytes.get(..HEADER_LEN).ok_or(ImageTableError::Corrupt)?;
        let version = header[8];
        let count = header[9] as usize;
        let entry_len = u16::from_le_bytes([header[10], header[11]]) as usize;
        if version == 0 || entry_len < ENTRY_LEN {
            return Err(ImageTableError::Version(version));
        }
        let len = HEADER_LEN + count * entry_len;
        let bytes = bytes.get(..len).ok_or(ImageTableError::Corrupt)?;
        let crc = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        if crc32(bytes, 8, len) != crc {
            return Err(ImageTableError::Corrupt);
        }
        Ok(ImageTable {
            bytes,
            count,
            entry_len,
        })
    }

    /// Scan an image's flash contents for its record: `Ok(None)` when it has none (an image from before the guard,
    /// or a board without reworks), an error when the one found is unusable or there are two.
    pub fn find(image: &'a [u8]) -> Result<Option<Self>, ImageTableError> {
        let mut found = None;
        let mut error = None;
        let mut at = 0;
        while let Some(pos) = image[at..].windows(8).position(|w| w == MAGIC) {
            let start = at + pos;
            match Self::parse(&image[start..]) {
                Ok(_) if found.is_some() => return Err(ImageTableError::Ambiguous),
                Ok(t) => found = Some(t),
                Err(e) => error = Some(e),
            }
            at = start + 1;
        }
        match (found, error) {
            (Some(t), _) => Ok(Some(t)),
            (None, Some(e)) => Err(e),
            (None, None) => Ok(None),
        }
    }

    /// The board revisions the image knows, in record order.
    pub fn entries(&self) -> impl Iterator<Item = ImageEntry<'a>> + use<'a> {
        let (bytes, entry_len) = (self.bytes, self.entry_len);
        (0..self.count).map(move |i| {
            let e = &bytes[HEADER_LEN + i * entry_len..];
            ImageEntry {
                name: text(&e[..NAME_LEN]),
                revision: text(&e[NAME_LEN..NAME_LEN + REVISION_LEN]),
                known: ReworkSet(u64::from_le_bytes(
                    e[NAME_LEN + REVISION_LEN..ENTRY_LEN]
                        .try_into()
                        .unwrap_or([0; 8]),
                )),
            }
        })
    }

    /// The reworks known for a board revision, `None` when the image doesn't list it.
    pub fn known(&self, name: &str, revision: &str) -> Option<ReworkSet> {
        self.entries()
            .find(|e| e.name == name && e.revision == revision)
            .map(|e| e.known)
    }

    /// The image lists some revision of this board.
    pub fn has_board(&self, name: &str) -> bool {
        self.entries().any(|e| e.name == name)
    }
}

/// One board revision of an [ImageTable].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageEntry<'a> {
    pub name: &'a str,
    pub revision: &'a str,
    pub known: ReworkSet,
}

/// What a flasher found comparing a board with a new image, see [check].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// The image knows every rework applied: flash, and clear a stored allowance.
    Ok,
    /// The image doesn't know these applied reworks: it will boot guarded unless the user agrees and an allowance is
    /// stored for it.
    Lacking(ReworkSet),
    /// The image lists other boards only: probably the wrong image. `applied` are the board's reworks.
    OtherBoard { applied: ReworkSet },
    /// The image has no record: built before the guard (or for a board without reworks). It doesn't guard, so it
    /// would run the board as built, ignoring `applied`; nothing to warn about when that is empty.
    NoRecord { applied: ReworkSet },
}

/// Compare a board (name, revision and applied reworks, as `BOARD` and `REWORKS` report them) with an image's
/// record: the one rule every flasher follows (TOOL-29).
pub fn check(image: Option<&ImageTable>, name: &str, revision: &str, applied: ReworkSet) -> Check {
    let Some(image) = image else {
        return if applied.is_empty() {
            Check::Ok
        } else {
            Check::NoRecord { applied }
        };
    };
    if !image.has_board(name) {
        return Check::OtherBoard { applied };
    }
    // a revision the image doesn't list: it knows none of its reworks
    let known = image.known(name, revision).unwrap_or(ReworkSet::EMPTY);
    let lacking = ReworkSet(applied.0 & !known.0);
    if lacking.is_empty() {
        Check::Ok
    } else {
        Check::Lacking(lacking)
    }
}

fn text(field: &[u8]) -> &str {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    core::str::from_utf8(&field[..end]).unwrap_or("")
}

const fn copy(dst: &mut [u8], at: usize, src: &[u8]) {
    let mut i = 0;
    while i < src.len() {
        dst[at + i] = src[i];
        i += 1;
    }
}

const fn eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// CRC-32 (IEEE 802.3, reflected, as zlib) of `bytes[from..to]` except the CRC field 12..16.
const fn crc32(bytes: &[u8], from: usize, to: usize) -> u32 {
    let mut crc = !0u32;
    let mut i = from;
    while i < to {
        if i < 12 || i >= 16 {
            crc ^= bytes[i] as u32;
            let mut k = 0;
            while k < 8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
                k += 1;
            }
        }
        i += 1;
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReworkId;
    use crate::table::ReworkDef;
    extern crate std;
    use std::vec::Vec;

    const B200A: ReworkTable = ReworkTable::new(
        &[
            ReworkDef::new(1, "a"),
            ReworkDef::new(5, "b"),
            ReworkDef::new(8, "c"),
        ],
        &[],
    );
    const NONE: ReworkTable = ReworkTable::new(&[], &[]);
    crate::image_reworks!(static RECORD = [("B200", "A", B200A), ("B200", "B", NONE)]);
    crate::image_reworks!(static EMPTY = []);

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    fn image(record: &[u8]) -> Vec<u8> {
        let mut v = std::vec![0xff; 300];
        v.extend_from_slice(record);
        v.extend([0u8; 77]);
        v
    }

    #[test]
    fn layout() {
        let b = RECORD.as_bytes();
        assert_eq!(b.len(), 16 + 2 * 48);
        assert_eq!(&b[..8], b"WWREWORK");
        assert_eq!(&b[8..12], &[1, 2, 48, 0]);
        assert_eq!(&b[16..20], b"B200");
        assert_eq!(b[48], b'A');
        assert_eq!(&b[56..64], &set(&[1, 5, 8]).0.to_le_bytes());
        // the CRC is zlib's CRC-32 of the header fields and the entries
        let mut covered = b[8..12].to_vec();
        covered.extend_from_slice(&b[16..]);
        let crc = u32::from_le_bytes(b[12..16].try_into().unwrap());
        let mut h = crc32_ref(&covered);
        assert_eq!(crc, h);
        h ^= 1;
        assert_ne!(crc, h);
        assert_eq!(EMPTY.as_bytes().len(), 16);
    }

    /// Plain table-free CRC-32 to check the const one against.
    fn crc32_ref(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
            }
        }
        !crc
    }

    #[test]
    fn known_from_the_record() {
        assert_eq!(RECORD.known("B200", "A"), set(&[1, 5, 8]));
        assert_eq!(RECORD.known("B200", "B"), ReworkSet::EMPTY);
        assert_eq!(RECORD.known("B200", "C"), ReworkSet::EMPTY);
        assert_eq!(RECORD.known("B135", "A"), ReworkSet::EMPTY);
    }

    #[test]
    fn found_in_image_bytes() {
        let img = image(RECORD.as_bytes());
        let t = ImageTable::find(&img).unwrap().unwrap();
        let entries: Vec<_> = t.entries().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            ImageEntry {
                name: "B200",
                revision: "A",
                known: set(&[1, 5, 8])
            }
        );
        assert_eq!(ImageTable::find(&[0xff; 100]), Ok(None));
    }

    #[test]
    fn corrupt_and_ambiguous() {
        let mut img = image(RECORD.as_bytes());
        img[300 + 20] ^= 1;
        assert_eq!(ImageTable::find(&img), Err(ImageTableError::Corrupt));
        // truncated
        let img = image(&RECORD.as_bytes()[..40]);
        assert_eq!(ImageTable::find(&img[..340]), Err(ImageTableError::Corrupt));
        // a stray magic next to a good record doesn't hide it
        let mut img = image(RECORD.as_bytes());
        img[10..18].copy_from_slice(b"WWREWORK");
        assert!(ImageTable::find(&img).unwrap().is_some());
        // two good records
        let mut img = image(RECORD.as_bytes());
        img.extend_from_slice(RECORD.as_bytes());
        assert_eq!(ImageTable::find(&img), Err(ImageTableError::Ambiguous));
    }

    #[test]
    fn newer_format_with_longer_entries_is_read() {
        // version 2, entries of 56 bytes: the first 48 are version 1's
        let mut rec = std::vec![0u8; 16 + 56];
        rec[..8].copy_from_slice(b"WWREWORK");
        rec[8] = 2;
        rec[9] = 1;
        rec[10] = 56;
        rec[16..20].copy_from_slice(b"B200");
        rec[48] = b'A';
        rec[56..64].copy_from_slice(&set(&[3]).0.to_le_bytes());
        let crc = crc32(&rec, 8, rec.len());
        rec[12..16].copy_from_slice(&crc.to_le_bytes());
        let t = ImageTable::parse(&rec).unwrap();
        assert_eq!(t.known("B200", "A"), Some(set(&[3])));
        rec[10] = 40;
        assert_eq!(ImageTable::parse(&rec), Err(ImageTableError::Version(2)));
    }

    #[test]
    fn checks() {
        let img = image(RECORD.as_bytes());
        let t = ImageTable::find(&img).unwrap().unwrap();
        assert_eq!(check(Some(&t), "B200", "A", set(&[1, 8])), Check::Ok);
        assert_eq!(
            check(Some(&t), "B200", "A", set(&[1, 9])),
            Check::Lacking(set(&[9]))
        );
        // a revision the image doesn't list knows none
        assert_eq!(
            check(Some(&t), "B200", "C", set(&[1])),
            Check::Lacking(set(&[1]))
        );
        assert_eq!(check(Some(&t), "B200", "C", set(&[])), Check::Ok);
        assert_eq!(
            check(Some(&t), "B135", "A", set(&[])),
            Check::OtherBoard { applied: set(&[]) }
        );
        assert_eq!(check(None, "B200", "A", set(&[])), Check::Ok);
        assert_eq!(
            check(None, "B200", "A", set(&[2])),
            Check::NoRecord { applied: set(&[2]) }
        );
    }
}
