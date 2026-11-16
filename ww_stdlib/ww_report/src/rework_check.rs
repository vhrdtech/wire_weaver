//! The rework check every flasher makes before writing an image (TOOL-29, `ww_board_info` STD-101).
//!
//! A firmware never runs a board with a rework it doesn't know: it refuses to boot and goes back to its ROM
//! bootloader. So before writing, a flasher compares the board's applied reworks with the reworks the new image
//! knows, and asks the user when the image lacks one. Only if they agree is an allowance stored for the new
//! image's [ImageId]; otherwise a stored allowance is cleared, so it can't outlive the image it was for.
//!
//! It all happens with the chip in its ROM bootloader, where a board that refuses to boot sits and a running one is
//! rebooted to first: the board's record is read straight from flash ([RomFlash], PICOBOOT on the RP2350, DFU on
//! an STM32), where the image table's [Locator] says.
//!
//! 1. [ImageReworks::from_flash] reads the new image's table and build id from the bytes about to be written.
//! 2. [scan_installed] finds the table of the image in flash: the board's name, and the locator when the new
//!    image has none.
//! 3. [read_board] reads the bench record and the allowance through the ROM.
//! 4. [verdict] compares ([ww_board_info::image::check]), [Verdict::needs_consent] says when to ask.
//! 5. [set_allowance] stores the allowance after the user agreed to [Verdict::Lacking], clears it otherwise; then
//!    the flasher writes the image.
//!
//! `ww info` and `ww board rework allow|disallow` on a device already in its boot ROM (TOOL-30) reuse the same
//! reads: [scan_installed_image] (unlike [scan_installed], answers even without a table, for the build id and the
//! boards an old image names), [read_board] and [ww_board_info::guard::decide] for the verdict a reboot would reach
//! right now ([rom_info_text] renders it; `wire_weaver_cli::cmd::rom` does the reads through PICOBOOT or DFU).
//!
//! ```ignore
//! let image = ImageReworks::from_flash(&flash_bytes);
//! let installed = scan_installed(&mut rom, FLASH_BASE, MAX_SCAN).await?;
//! let Some(board) = read_board(&mut rom, &image, installed.as_ref(), None).await? else { /* nothing to check */ };
//! match verdict(&image, &board) {
//!     Verdict::Ok => { set_allowance(&mut rom, &board, StoredAllowance::NONE).await?; }
//!     Verdict::Lacking { lacking, image: Some(id) } if user_agrees() => {
//!         set_allowance(&mut rom, &board, StoredAllowance::new(id, lacking)).await?;
//!     }
//!     other => { /* warn, ask or refuse */ }
//! }
//! ```

use std::fmt;
use std::fmt::Write;

use anyhow::{Result, anyhow, bail};
use ww_board_info::guard::{BenchRecord, Guard, ImageId, StoredAllowance};
use ww_board_info::image::{self, Check, ImageTable, ImageTableError, Locator};
use ww_board_info::record;
use ww_board_info::table::{IdRanges, ReworkSet};

pub use ww_settings::fake_flash::FlashOp;

/// Flash through a chip's ROM bootloader, at memory addresses.
pub trait RomFlash {
    /// Read `len` bytes at `addr`.
    fn read(&mut self, addr: u32, len: u32) -> impl Future<Output = Result<Vec<u8>>>;
    /// Erase and write, in this order: what [record::set_allowance] answers. Writes are within erased words or
    /// rewrite bytes as they are; an implementation pads them to its program unit with `0xFF`.
    fn apply(&mut self, ops: &[FlashOp]) -> impl Future<Output = Result<()>>;
}

/// What an image says about reworks: its table (`ww_board_info::image`) and its [ImageId].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageReworks {
    /// The table's bytes, `Ok(None)` for an image without one (from before the guard).
    record: Result<Option<Vec<u8>>, ImageTableError>,
    /// The first 8 bytes of the image's GNU build id; `None` without a build-id note: no allowance can be its own.
    pub id: Option<ImageId>,
}

impl ImageReworks {
    /// From the image's flash contents (segments of an ELF, UF2 or raw binary put one after the other, or what a
    /// ROM read of the flash gave).
    pub fn from_flash(bytes: &[u8]) -> Self {
        let record = ImageTable::find(bytes).map(|t| t.map(|t| t.as_bytes().to_vec()));
        let id = ww_firmware_info::find_build_id(bytes).and_then(ImageId::from_build_id);
        ImageReworks { record, id }
    }

    /// The image's table, `Ok(None)` when it has none.
    pub fn table(&self) -> Result<Option<ImageTable<'_>>, ImageTableError> {
        match &self.record {
            Ok(Some(bytes)) => ImageTable::parse(bytes).map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(*e),
        }
    }

    /// The board revisions the image lists, `B200 A, B200 B`.
    pub fn boards(&self) -> String {
        let Ok(Some(table)) = self.table() else {
            return String::new();
        };
        table
            .entries()
            .map(|e| format!("{} {}", e.name, e.revision))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The board names the image lists, each once.
    pub fn board_names(&self) -> Vec<String> {
        match self.table() {
            Ok(Some(t)) => t.board_names().map(String::from).collect(),
            _ => vec![],
        }
    }

    /// The image checks the board at boot: it has a usable table.
    pub fn guards(&self) -> bool {
        matches!(self.table(), Ok(Some(_)))
    }

    /// Where the image reads the board's record, `None` without a table or a record.
    pub fn locator(&self) -> Option<Locator> {
        match self.table() {
            Ok(Some(t)) if t.locator().has_record() => Some(t.locator()),
            _ => None,
        }
    }
}

/// Bytes [scan_installed] and [scan_installed_image] read at a time.
pub const SCAN_CHUNK: u32 = 64 * 1024;

/// The image's bytes from `flash_base`: read chunk by chunk until its table is found, a chunk reads all erased (the
/// image ended) or `max_len` is read.
async fn scan_bytes(rom: &mut impl RomFlash, flash_base: u32, max_len: u32) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut at = 0;
    while at < max_len {
        let len = SCAN_CHUNK.min(max_len - at);
        let chunk = rom.read(flash_base + at, len).await?;
        let erased = chunk.iter().all(|b| *b == 0xff);
        bytes.extend_from_slice(&chunk);
        at += len;
        // a table cut by the chunk's end doesn't parse yet: read on
        if let Ok(Some(_)) = ImageTable::find(&bytes) {
            break;
        }
        if erased {
            break;
        }
    }
    Ok(bytes)
}

/// Find the table of the image in flash from `flash_base`: read chunk by chunk until the table is found, a chunk
/// reads all erased (the image ended) or `max_len` is read. `None`: no table (an erased chip, an image from before
/// the guard).
pub async fn scan_installed(
    rom: &mut impl RomFlash,
    flash_base: u32,
    max_len: u32,
) -> Result<Option<ImageReworks>> {
    let found = ImageReworks::from_flash(&scan_bytes(rom, flash_base, max_len).await?);
    Ok(found
        .record
        .as_ref()
        .is_ok_and(|r| r.is_some())
        .then_some(found))
}

/// The image in flash from `flash_base`, whether it has a table or not: `ww info` (TOOL-30) wants the build id and
/// the boards an old image names too, where [scan_installed] answers `None`.
pub async fn scan_installed_image(
    rom: &mut impl RomFlash,
    flash_base: u32,
    max_len: u32,
) -> Result<ImageReworks> {
    Ok(ImageReworks::from_flash(
        &scan_bytes(rom, flash_base, max_len).await?,
    ))
}

/// The board as its record in flash says, see [read_board].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardReworks {
    pub name: String,
    /// The revision letter, `""` when the record doesn't set one.
    pub revision: String,
    /// Applied reworks.
    pub applied: ReworkSet,
    /// The allowance stored.
    pub allowance: StoredAllowance,
    /// Where it was read, and the store's bytes as read: what [set_allowance] changes.
    pub locator: Locator,
    store: Vec<u8>,
}

/// Why the board's name couldn't be told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Neither image names exactly one board: give it.
    Unknown { names: Vec<String> },
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Unknown { names } if names.is_empty() => {
                f.write_str("the board's name is unknown (no image names it): give it with --board")
            }
            NameError::Unknown { names } => write!(
                f,
                "the images name several boards ({}): give the board with --board",
                names.join(", ")
            ),
        }
    }
}

impl std::error::Error for NameError {}

/// The board's name: `given` (`--board`), else the one board the image in flash names, else the one the new image
/// names.
pub fn board_name(
    given: Option<&str>,
    installed: Option<&ImageReworks>,
    new: Option<&ImageReworks>,
) -> Result<String, NameError> {
    if let Some(name) = given {
        return Ok(name.to_string());
    }
    let mut names = vec![];
    for image in [installed, new].into_iter().flatten() {
        match image.board_names().as_slice() {
            [one] => return Ok(one.clone()),
            many => names.extend(many.iter().cloned()),
        }
    }
    names.dedup();
    Err(NameError::Unknown { names })
}

/// Read the board's bench record and allowance through the ROM, where the new image's locator says (else the one
/// of the image in flash). `Ok(None)`: neither image names a record, nothing to check.
pub async fn read_board(
    rom: &mut impl RomFlash,
    new: &ImageReworks,
    installed: Option<&ImageReworks>,
    given_name: Option<&str>,
) -> Result<Option<BoardReworks>> {
    let Some(locator) = new
        .locator()
        .or_else(|| installed.and_then(|i| i.locator()))
    else {
        return Ok(None);
    };
    let addr = locator.store_addr();
    let store = rom.read(addr.start, locator.store_len).await?;
    let bench: BenchRecord = record::read(&locator, &store)
        .map_err(|e| anyhow!("reading the board's record at {:#010x}: {e}", addr.start))?;
    let name = board_name(given_name, installed, Some(new))?;
    Ok(Some(BoardReworks {
        name,
        revision: bench.revision_str().to_string(),
        applied: bench.reworks,
        allowance: bench.allowance,
        locator,
        store,
    }))
}

/// Store `allowance` ([StoredAllowance::NONE]: clear it) through the ROM. `false`: it was stored already, nothing
/// written.
pub async fn set_allowance(
    rom: &mut impl RomFlash,
    board: &BoardReworks,
    allowance: StoredAllowance,
) -> Result<bool> {
    let change = record::set_allowance(&board.locator, &board.store, allowance)?;
    if change.ops.is_empty() {
        return Ok(false);
    }
    rom.apply(&change.ops).await?;
    // read back: the ROM wrote what the store would have
    let addr = board.locator.store_addr();
    let read = rom.read(addr.start, board.locator.store_len).await?;
    if read != change.image {
        bail!(
            "the board's settings store reads back different after writing the allowance ({:#010x})",
            addr.start
        );
    }
    Ok(true)
}

/// How a new image fits the board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The image knows every applied rework (or has no table and the board none applied): write it, clear a
    /// stored allowance.
    Ok,
    /// The image doesn't know these applied reworks: it refuses to boot unless the user agrees and an allowance is
    /// stored for `image` (`None`: the image has no build id, no allowance can be its own).
    Lacking {
        lacking: ReworkSet,
        image: Option<ImageId>,
    },
    /// The image lists other boards only: probably the wrong image.
    OtherBoard { boards: String },
    /// The image has no table (built before the guard): it would run the board as built, ignoring these applied
    /// reworks.
    NoRecord { applied: ReworkSet },
    /// The image's table is unusable: the image would refuse to boot on any board with a rework applied.
    BadRecord(ImageTableError),
}

impl Verdict {
    /// The user must agree before the image is written.
    pub fn needs_consent(&self) -> bool {
        !matches!(self, Verdict::Ok)
    }
}

/// Compare the board with the image: [ww_board_info::image::check], with what the flasher needs to say.
pub fn verdict(image: &ImageReworks, board: &BoardReworks) -> Verdict {
    let table = match image.table() {
        Ok(t) => t,
        // nothing applied: no rework the table could lack
        Err(_) if board.applied.is_empty() => return Verdict::Ok,
        Err(e) => return Verdict::BadRecord(e),
    };
    match image::check(table.as_ref(), &board.name, &board.revision, board.applied) {
        Check::Ok => Verdict::Ok,
        Check::Lacking(lacking) => Verdict::Lacking {
            lacking,
            image: image.id,
        },
        Check::OtherBoard { .. } => Verdict::OtherBoard {
            boards: image.boards(),
        },
        Check::NoRecord { applied } => Verdict::NoRecord { applied },
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Ok => f.write_str("the image knows every rework applied on the board"),
            Verdict::Lacking {
                lacking,
                image: Some(id),
            } => write!(
                f,
                "the image doesn't know rework(s) {} applied on the board: it refuses to boot unless they are allowed for image {id}",
                IdRanges(*lacking)
            ),
            Verdict::Lacking {
                lacking,
                image: None,
            } => write!(
                f,
                "the image doesn't know rework(s) {} applied on the board, and has no build id to allow them for: it refuses to boot",
                IdRanges(*lacking)
            ),
            Verdict::OtherBoard { boards } => {
                write!(f, "the image is for {boards}, not this board")
            }
            Verdict::NoRecord { applied } => write!(
                f,
                "the image has no rework table (built before the rework guard): it runs the board as built, ignoring rework(s) {}",
                IdRanges(*applied)
            ),
            Verdict::BadRecord(e) => write!(f, "{e}: it refuses to boot"),
        }
    }
}

/// The allowance, one line: `none`, `rework(s) 9 for image 3fa2..`.
pub fn allowance_line(allowance: &StoredAllowance) -> String {
    match allowance.image_id() {
        None => "none".to_string(),
        Some(id) => format!("rework(s) {} for image {id}", IdRanges(allowance.reworks)),
    }
}

/// Whether the installed image would run the board right now ([ww_board_info::guard::decide]), one line, see
/// [rom_info_text].
pub fn guard_line(guard: &Guard) -> String {
    match guard {
        Guard::Run => "runs: every applied rework is known".to_string(),
        Guard::RunAllowed { unknown } => format!(
            "runs: rework(s) {} unknown to this image, allowed for it",
            IdRanges(*unknown)
        ),
        Guard::Refuse { unknown } => {
            let numbers = unknown
                .iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>();
            format!(
                "refuses to boot: unknown rework(s) {} (blink code 1); flash a firmware that knows {}, or `ww board rework allow {}`",
                IdRanges(*unknown),
                if numbers.len() == 1 { "it" } else { "them" },
                numbers.join(",")
            )
        }
    }
}

/// What `ww info` shows for a device sitting in its boot ROM (TOOL-30): the image in flash, the board's bench
/// record and the allowance, and the verdict a reboot would reach right now. Pure text from what [scan_installed_image]
/// and [read_board] answered, so it is tested without a device (`RamRom` below, test-only).
pub fn rom_info_text(image: &ImageReworks, board: Option<&BoardReworks>) -> String {
    let mut out = String::new();
    if image.guards() {
        let _ = write!(
            out,
            "Image      {}",
            match image.id {
                Some(id) => format!("build id {id}"),
                None => "no build id (the image keeps none)".to_string(),
            }
        );
        let boards = image.boards();
        if !boards.is_empty() {
            let _ = write!(out, ", boards {boards}");
        }
        out.push('\n');
    } else {
        out.push_str(
            "Image      no rework table (built before the rework guard, or flash has no image)\n",
        );
    }
    let Some(board) = board else {
        out.push_str("Board      no bench record: nothing to check\n");
        return out;
    };
    let _ = writeln!(
        out,
        "Board      {} rev {}, reworks applied {}",
        if board.name.is_empty() {
            "?"
        } else {
            &board.name
        },
        if board.revision.is_empty() {
            "?"
        } else {
            &board.revision
        },
        if board.applied.is_empty() {
            "none".to_string()
        } else {
            IdRanges(board.applied).to_string()
        },
    );
    let _ = writeln!(out, "Allowance  {}", allowance_line(&board.allowance));
    let _ = writeln!(out, "Verdict    {}", guard_line(&board_guard(image, board)));
    out
}

/// What a reboot would decide right now for `board` under `image` ([ww_board_info::guard::decide]), what
/// [rom_info_text] and `wire_weaver_cli`'s `--format json` for `ww info` in the ROM show.
pub fn board_guard(image: &ImageReworks, board: &BoardReworks) -> Guard {
    let known = image
        .table()
        .ok()
        .flatten()
        .and_then(|t| t.known(&board.name, &board.revision))
        .unwrap_or(ReworkSet::EMPTY);
    ww_board_info::guard::decide(known, board.applied, board.allowance, image.id)
}

/// A [RomFlash] in RAM for tests: `flash` at `base`, with page erases of `page` bytes.
#[cfg(test)]
pub(crate) struct RamRom {
    pub base: u32,
    pub page: u32,
    pub flash: Vec<u8>,
    pub reads: usize,
}

#[cfg(test)]
impl RomFlash for RamRom {
    async fn read(&mut self, addr: u32, len: u32) -> Result<Vec<u8>> {
        self.reads += 1;
        let at = (addr - self.base) as usize;
        Ok(self.flash[at..at + len as usize].to_vec())
    }

    async fn apply(&mut self, ops: &[FlashOp]) -> Result<()> {
        for op in ops {
            match op {
                FlashOp::Erase { from, to } => {
                    assert!(from % self.page == 0 && to % self.page == 0);
                    self.flash[(from - self.base) as usize..(to - self.base) as usize].fill(0xff);
                }
                FlashOp::Write { offset, bytes } => {
                    let at = (offset - self.base) as usize;
                    for (d, s) in self.flash[at..at + bytes.len()].iter_mut().zip(bytes) {
                        *d &= *s;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_board_info::ReworkId;
    use ww_board_info::table::{ReworkDef, ReworkTable};

    const BASE: u32 = 0x1000_0000;
    const LOC: Locator = Locator {
        flash_base: BASE,
        store_start: 0x7c000,
        store_len: 0x4000,
        page_size: 4096,
        word_size: 1,
        record_key: 1,
        revision_slot: 0,
        reworks_slot: 1,
        reworks_width: 4,
        allowance_key: 2,
    };
    const B200B: ReworkTable =
        ReworkTable::new(&[ReworkDef::new(1, "a"), ReworkDef::new(3, "b")], &[]);
    ww_board_info::image_reworks!(static RECORD = LOC, [("B200", "B", B200B)]);

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    /// An image's flash contents: code, the build-id note, the table.
    fn flash(record: bool, build_id: u8) -> Vec<u8> {
        let mut v = vec![0x11; 64];
        if build_id != 0 {
            v.extend([4, 0, 0, 0, 20, 0, 0, 0, 3, 0, 0, 0]);
            v.extend(b"GNU\0");
            v.extend((0..20u8).map(|i| build_id.wrapping_add(i)));
        }
        if record {
            v.extend(RECORD.as_bytes());
        }
        v.extend([0; 32]);
        v
    }

    fn board(name: &str, revision: &str, applied: &[u16]) -> BoardReworks {
        BoardReworks {
            name: name.into(),
            revision: revision.into(),
            applied: set(applied),
            allowance: StoredAllowance::NONE,
            locator: LOC,
            store: vec![],
        }
    }

    /// A chip: the installed image at the start of flash, the store as the device left it.
    fn chip(installed: &[u8], revision: u8, reworks: u32) -> RamRom {
        let mut flash = vec![0xff; 0x80000];
        flash[..installed.len()].copy_from_slice(installed);
        // the record as the board's firmware writes it (`ww_board_info::record` writes the same way)
        #[derive(ww_settings::Settings, Clone, Copy, Debug, PartialEq, Eq)]
        struct Facts {
            #[setting(slot = 0, default = 0)]
            revision: u8,
            #[setting(slot = 1, default = 0)]
            reworks: u32,
        }
        let mut fake = ww_settings::fake_flash::FakeFlash::<1, 4096>::new(LOC.store_start, 4);
        embassy_futures::block_on(async {
            let mut store: ww_settings::Store<_, 64, 4> =
                ww_settings::Store::new(&mut fake, LOC.store()).unwrap();
            let mut f: ww_settings::Settings<Facts, 64> =
                ww_settings::Settings::new(ww_settings::Key(1), Default::default());
            f.set(0, |f| *f = Facts { revision, reworks });
            f.persist(&mut store).await.unwrap();
        });
        flash[LOC.store_start as usize..].copy_from_slice(fake.image());
        RamRom {
            base: BASE,
            page: 4096,
            flash,
            reads: 0,
        }
    }

    #[test]
    fn image_id_and_table_from_flash() {
        let image = ImageReworks::from_flash(&flash(true, 0x10));
        assert_eq!(
            image.id,
            Some(ImageId([0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]))
        );
        assert!(image.guards());
        assert_eq!(image.boards(), "B200 B");
        assert_eq!(image.locator(), Some(LOC));
        let old = ImageReworks::from_flash(&flash(false, 0));
        assert_eq!(old.id, None);
        assert!(!old.guards());
        assert_eq!(old.locator(), None);
    }

    #[test]
    fn verdicts() {
        let image = ImageReworks::from_flash(&flash(true, 0x10));
        assert_eq!(verdict(&image, &board("B200", "B", &[1, 3])), Verdict::Ok);
        assert_eq!(
            verdict(&image, &board("B200", "B", &[1, 7, 9])),
            Verdict::Lacking {
                lacking: set(&[7, 9]),
                image: image.id
            }
        );
        // a revision the image doesn't list: it knows none of its reworks
        assert_eq!(
            verdict(&image, &board("B200", "A", &[1])),
            Verdict::Lacking {
                lacking: set(&[1]),
                image: image.id
            }
        );
        assert_eq!(verdict(&image, &board("B200", "A", &[])), Verdict::Ok);
        assert_eq!(
            verdict(&image, &board("B135", "A", &[])),
            Verdict::OtherBoard {
                boards: "B200 B".into()
            }
        );

        let old = ImageReworks::from_flash(&flash(false, 0x10));
        assert_eq!(verdict(&old, &board("B200", "B", &[])), Verdict::Ok);
        assert_eq!(
            verdict(&old, &board("B200", "B", &[3])),
            Verdict::NoRecord { applied: set(&[3]) }
        );

        let no_id = ImageReworks::from_flash(&flash(true, 0));
        assert_eq!(
            verdict(&no_id, &board("B200", "B", &[5])),
            Verdict::Lacking {
                lacking: set(&[5]),
                image: None
            }
        );
    }

    #[test]
    fn corrupt_table() {
        let mut bytes = flash(true, 0x10);
        let at = bytes.len() - 32 - 8;
        bytes[at] ^= 1; // in the known reworks of the entry: the CRC no longer matches
        let image = ImageReworks::from_flash(&bytes);
        assert!(!image.guards());
        assert_eq!(
            verdict(&image, &board("B200", "B", &[1])),
            Verdict::BadRecord(ImageTableError::Corrupt)
        );
        assert_eq!(verdict(&image, &board("B200", "B", &[])), Verdict::Ok);
    }

    #[test]
    fn names() {
        let one = ImageReworks::from_flash(&flash(true, 0x10));
        let none = ImageReworks::from_flash(&flash(false, 0x10));
        assert_eq!(board_name(Some("B135"), Some(&one), None).unwrap(), "B135");
        assert_eq!(board_name(None, Some(&one), Some(&none)).unwrap(), "B200");
        assert_eq!(board_name(None, None, Some(&one)).unwrap(), "B200");
        assert_eq!(
            board_name(None, Some(&none), None),
            Err(NameError::Unknown { names: vec![] })
        );
    }

    #[test]
    fn through_the_rom() {
        // the image in flash knows rework 1 and 3 of rev B; the board has 3 and 9 applied
        let mut rom = chip(&flash(true, 0x10), b'B', 1 << 3 | 1 << 9);
        let new = ImageReworks::from_flash(&flash(true, 0x40));
        embassy_futures::block_on(async {
            let installed = scan_installed(&mut rom, BASE, 0x80000)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(installed.id.unwrap().0[0], 0x10);
            // found in the first chunk
            assert_eq!(rom.reads, 1);
            let b = read_board(&mut rom, &new, Some(&installed), None)
                .await
                .unwrap()
                .unwrap();
            assert_eq!((b.name.as_str(), b.revision.as_str()), ("B200", "B"));
            assert_eq!(b.applied, set(&[3, 9]));
            assert_eq!(b.allowance, StoredAllowance::NONE);
            let v = verdict(&new, &b);
            assert_eq!(
                v,
                Verdict::Lacking {
                    lacking: set(&[9]),
                    image: new.id
                }
            );

            // agreed: stored for the new image, read back the same way
            let allowance = StoredAllowance::new(new.id.unwrap(), set(&[9]));
            assert!(set_allowance(&mut rom, &b, allowance).await.unwrap());
            let b = read_board(&mut rom, &new, Some(&installed), None)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(b.allowance, allowance);
            assert!(!set_allowance(&mut rom, &b, allowance).await.unwrap());
            // the next flashing that needs none clears it
            assert!(
                set_allowance(&mut rom, &b, StoredAllowance::NONE)
                    .await
                    .unwrap()
            );
        });
    }

    #[test]
    fn scan_stops_at_erased_flash() {
        let mut rom = chip(&flash(false, 0x10), b'B', 0);
        embassy_futures::block_on(async {
            assert_eq!(scan_installed(&mut rom, BASE, 0x80000).await.unwrap(), None);
        });
        // the first chunk has the old image, the second reads erased
        assert_eq!(rom.reads, 2);
    }

    #[test]
    fn scan_installed_image_without_a_table() {
        // an image from before the guard: no table, but scan_installed_image still gives the build id
        let mut rom = chip(&flash(false, 0x10), b'B', 0);
        embassy_futures::block_on(async {
            let image = scan_installed_image(&mut rom, BASE, 0x80000).await.unwrap();
            assert!(!image.guards());
            assert_eq!(image.id.unwrap().0[0], 0x10);
            assert_eq!(scan_installed(&mut rom, BASE, 0x80000).await.unwrap(), None);
        });
    }

    #[test]
    fn rom_info_text_states() {
        embassy_futures::block_on(async {
            // runs: every applied rework known
            let mut rom = chip(&flash(true, 0x10), b'B', 1 << 1);
            let image = scan_installed_image(&mut rom, BASE, 0x80000).await.unwrap();
            let board = read_board(&mut rom, &image, None, None).await.unwrap();
            let text = rom_info_text(&image, board.as_ref());
            assert!(
                text.contains("Verdict    runs: every applied rework is known"),
                "{text}"
            );
            assert!(
                text.contains(&format!("Image      build id {}", image.id.unwrap())),
                "{text}"
            );
            assert!(
                text.contains("Board      B200 rev B, reworks applied 1"),
                "{text}"
            );

            // refuses to boot: rework 9 applied, unknown, no allowance
            let mut rom = chip(&flash(true, 0x10), b'B', 1 << 3 | 1 << 9);
            let image = scan_installed_image(&mut rom, BASE, 0x80000).await.unwrap();
            let board = read_board(&mut rom, &image, None, None)
                .await
                .unwrap()
                .unwrap();
            let text = rom_info_text(&image, Some(&board));
            assert!(
                text.contains("refuses to boot: unknown rework(s) 9 (blink code 1)"),
                "{text}"
            );
            assert!(text.contains("ww board rework allow 9"), "{text}");

            // runs with the allowance, once stored
            let allowance = StoredAllowance::new(image.id.unwrap(), set(&[9]));
            set_allowance(&mut rom, &board, allowance).await.unwrap();
            let board = read_board(&mut rom, &image, None, None)
                .await
                .unwrap()
                .unwrap();
            let text = rom_info_text(&image, Some(&board));
            assert!(
                text.contains("runs: rework(s) 9 unknown to this image, allowed for it"),
                "{text}"
            );

            // an image from before the guard: no table, nothing to check
            let old = ImageReworks::from_flash(&flash(false, 0x10));
            let text = rom_info_text(&old, None);
            assert!(text.starts_with("Image      no rework table"), "{text}");
            assert!(text.contains("Board      no bench record"), "{text}");
        });
    }

    #[test]
    fn messages() {
        let id = ImageId([0x3f, 0xa2, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            Verdict::Lacking {
                lacking: set(&[7, 9]),
                image: Some(id)
            }
            .to_string(),
            "the image doesn't know rework(s) 7, 9 applied on the board: it refuses to boot unless they are allowed for image 3fa2010203040506"
        );
        assert_eq!(
            Verdict::NoRecord { applied: set(&[3]) }.to_string(),
            "the image has no rework table (built before the rework guard): it runs the board as built, ignoring rework(s) 3"
        );
        assert_eq!(allowance_line(&StoredAllowance::NONE), "none");
        assert_eq!(
            allowance_line(&StoredAllowance::new(id, set(&[9]))),
            "rework(s) 9 for image 3fa2010203040506"
        );
    }
}
