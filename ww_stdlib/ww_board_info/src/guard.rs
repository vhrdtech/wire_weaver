//! The rework guard (STD-101): a firmware never runs a board with a rework it doesn't know.
//!
//! At the very start of `main`, before any board init (no HAL, no pin, no peripheral clock), the firmware calls
//! [boot_check]: it reads the bench record and the allowance where its image table's [Locator] says (the same
//! place a flasher reads through the ROM bootloader), and [decide]s with the reworks the image knows for the
//! board revision. The answer is one of:
//!
//! * [Guard::Run]: every applied rework is known, run the board.
//! * [Guard::RunAllowed]: unknown reworks, but the allowance for this very image covers them: run the board.
//! * [Guard::Refuse]: an applied rework is unknown and no allowance for this image covers it. Don't init anything:
//!   blink the error code (`bedrock_led::code`) on the status LED, then reboot into the ROM bootloader, where a
//!   flasher can act.
//!
//! The allowance is two `u64` ([StoredAllowance]), an [AllowanceGroup] in the board's settings store. Only `ww`
//! writes it, through the ROM bootloader, for the image it is about to flash ([crate::record]); the firmware only
//! reads it, and the next `ww` flashing that needs none clears it.
//!
//! ```
//! use ww_board_info::{ReworkId, guard::{decide, Guard, ImageId, StoredAllowance}, table::{ReworkDef, ReworkSet, ReworkTable}};
//!
//! const TABLE: ReworkTable = ReworkTable::new(&[ReworkDef::new(1, "analog buffer supply")], &[]);
//! let me = ImageId([1, 2, 3, 4, 5, 6, 7, 8]);
//! let applied = ReworkSet::EMPTY.with(ReworkId(1), true).with(ReworkId(9), true);
//!
//! let guard = decide(TABLE.known(), applied, StoredAllowance::NONE, Some(me));
//! assert_eq!(guard, Guard::Refuse { unknown: ReworkSet::EMPTY.with(ReworkId(9), true) });
//!
//! let allowance = StoredAllowance::new(me, ReworkSet::EMPTY.with(ReworkId(9), true));
//! assert!(matches!(decide(TABLE.known(), applied, allowance, Some(me)), Guard::RunAllowed { .. }));
//!
//! // the same allowance is void for any other image
//! let other = decide(TABLE.known(), applied, allowance, Some(ImageId([9; 8])));
//! assert!(matches!(other, Guard::Refuse { .. }));
//! ```

use core::fmt;

use embedded_storage_async::nor_flash::NorFlash;
use ww_settings::{Debounce, Key, Settings, Store, StoreError, record::Record};

use crate::ReworkId;
use crate::image::{ImageTable, Locator};
use crate::table::ReworkSet;
#[cfg(doc)]
use crate::table::ReworkTable;

/// Which image an allowance is for: the first 8 bytes of its GNU build id (`ww_firmware_info::build_id`), the same
/// in the image file (`ww_firmware_info::find_build_id`) and in the running firmware. Not a security boundary:
/// whoever can store an allowance can also change the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ImageId(pub [u8; 8]);

impl ImageId {
    /// The id of the image with this build id; `None` when it is shorter than 8 bytes (no build-id note), or all
    /// zeros, which a stored allowance uses for "none".
    pub fn from_build_id(build_id: &[u8]) -> Option<ImageId> {
        let id: [u8; 8] = build_id.get(..8)?.try_into().ok()?;
        (id != [0; 8]).then_some(ImageId(id))
    }
}

impl fmt::Display for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// An allowance as a board stores it, see [AllowanceGroup].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct StoredAllowance {
    /// [ImageId] as a little-endian `u64`; 0: no allowance.
    pub image: u64,
    /// The unknown reworks it covers.
    pub reworks: ReworkSet,
}

impl StoredAllowance {
    pub const NONE: StoredAllowance = StoredAllowance {
        image: 0,
        reworks: ReworkSet::EMPTY,
    };

    pub const fn new(image: ImageId, reworks: ReworkSet) -> Self {
        StoredAllowance {
            image: u64::from_le_bytes(image.0),
            reworks,
        }
    }

    /// The allowance for `image` covering these numbers, or the number that can't be in a record (past
    /// [ReworkSet::BITS]).
    pub fn for_reworks(
        image: ImageId,
        reworks: impl IntoIterator<Item = ReworkId>,
    ) -> Result<Self, ReworkId> {
        let mut set = ReworkSet::EMPTY;
        for id in reworks {
            if id.0 as u32 >= ReworkSet::BITS {
                return Err(id);
            }
            set = set.with(id, true);
        }
        Ok(StoredAllowance::new(image, set))
    }

    pub const fn is_none(&self) -> bool {
        self.image == 0
    }

    /// The image it is for, `None` when there is no allowance.
    pub const fn image_id(&self) -> Option<ImageId> {
        if self.image == 0 {
            None
        } else {
            Some(ImageId(self.image.to_le_bytes()))
        }
    }

    /// It is for `image` and covers every number of `unknown`.
    pub fn covers(&self, image: Option<ImageId>, unknown: ReworkSet) -> bool {
        match (self.image_id(), image) {
            (Some(stored), Some(image)) => stored == image && unknown.0 & !self.reworks.0 == 0,
            _ => false,
        }
    }
}

/// The allowance as a `ww_settings` group in the board's settings store, under the key the image's [Locator] names
/// (`allowance_key`). All defaults: none.
#[derive(ww_settings::Settings, Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AllowanceGroup {
    /// [StoredAllowance::image]
    #[setting(slot = 0, default = 0)]
    pub image: u64,
    /// [StoredAllowance::reworks], bit n = rework n
    #[setting(slot = 1, default = 0)]
    pub reworks: u64,
}

impl From<AllowanceGroup> for StoredAllowance {
    fn from(g: AllowanceGroup) -> Self {
        StoredAllowance {
            image: g.image,
            reworks: ReworkSet(g.reworks),
        }
    }
}

impl From<StoredAllowance> for AllowanceGroup {
    fn from(a: StoredAllowance) -> Self {
        AllowanceGroup {
            image: a.image,
            reworks: a.reworks.0,
        }
    }
}

/// Whether the board may run, see the [module](self) docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Guard {
    Run,
    RunAllowed { unknown: ReworkSet },
    Refuse { unknown: ReworkSet },
}

impl Guard {
    /// The board may be initialised and run ([Guard::Run] or [Guard::RunAllowed]).
    pub const fn runs_board(&self) -> bool {
        !matches!(self, Guard::Refuse { .. })
    }

    /// The applied reworks the firmware doesn't know.
    pub const fn unknown(&self) -> ReworkSet {
        match self {
            Guard::Run => ReworkSet::EMPTY,
            Guard::Refuse { unknown } | Guard::RunAllowed { unknown } => *unknown,
        }
    }
}

/// The decision. `known`: the reworks the image knows for the board revision ([ReworkTable::known]; in firmware
/// [crate::image::ImageReworks::known], empty for a revision the image doesn't list). `own` is the running image's
/// id ([ImageId::from_build_id] of `ww_firmware_info::build_id()`); `None` (no build-id note) never matches an
/// allowance, so such an image refuses an unknown rework whatever is stored.
pub fn decide(
    known: ReworkSet,
    applied: ReworkSet,
    allowance: StoredAllowance,
    own: Option<ImageId>,
) -> Guard {
    let unknown = ReworkSet(applied.0 & !known.0);
    if unknown.is_empty() {
        Guard::Run
    } else if allowance.covers(own, unknown) {
        Guard::RunAllowed { unknown }
    } else {
        Guard::Refuse { unknown }
    }
}

/// The board's bench record and the allowance, as the [Locator] finds them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct BenchRecord {
    /// The revision letter, `None` when the record doesn't set one (or there is no record).
    pub revision: Option<u8>,
    /// Reworks applied, empty without a record.
    pub reworks: ReworkSet,
    pub allowance: StoredAllowance,
}

impl BenchRecord {
    /// The revision as the image table writes it, `""` when not set.
    pub fn revision_str(&self) -> &str {
        match &self.revision {
            Some(r) => core::str::from_utf8(core::slice::from_ref(r)).unwrap_or(""),
            None => "",
        }
    }
}

/// Why the bench record couldn't be read. A firmware refuses to boot on it as on an unknown rework: it can't tell
/// what the board is.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadError<E> {
    /// The locator's range doesn't fit the store (not whole pages of the flash's size, fewer than 2).
    Locator,
    /// The flash failed.
    Store(StoreError<E>),
    /// The record (or the allowance) is from a newer format, or doesn't decode.
    Unreadable,
}

impl<E> From<StoreError<E>> for ReadError<E> {
    fn from(e: StoreError<E>) -> Self {
        ReadError::Store(e)
    }
}

/// Most bytes of the bench record a reader looks at: it carries a revision and a set of reworks, a few bytes.
pub const MAX_RECORD: usize = 128;

/// Read the bench record and the allowance from `store` (over the range [Locator::store] of the board's flash).
/// No record: an empty one.
pub async fn read_bench<S: NorFlash, const B: usize, const K: usize>(
    store: &mut Store<S, B, K>,
    locator: &Locator,
) -> Result<BenchRecord, ReadError<S::Error>> {
    if !locator.has_record() {
        return Ok(BenchRecord::default());
    }
    let mut buf = [0u8; MAX_RECORD];
    let mut bench = BenchRecord::default();
    if let Some(len) = store.fetch(Key(locator.record_key), &mut buf).await? {
        let record = Record::parse(&buf[..len]).map_err(|_| ReadError::Unreadable)?;
        bench.revision = slot::<u8, _>(&record, locator.revision_slot)?.filter(|&r| r != 0);
        bench.reworks = ReworkSet(match locator.reworks_width {
            4 => slot::<u32, _>(&record, locator.reworks_slot)?.unwrap_or(0) as u64,
            _ => slot::<u64, _>(&record, locator.reworks_slot)?.unwrap_or(0),
        });
    }
    let mut allowance: Settings<AllowanceGroup, MAX_RECORD> =
        Settings::new(Key(locator.allowance_key), Debounce::default());
    let report = allowance.load(store).await.map_err(|e| match e {
        ww_settings::Error::Store(e) => ReadError::Store(e),
        _ => ReadError::Unreadable,
    })?;
    if report.unreadable.is_some() {
        return Err(ReadError::Unreadable);
    }
    bench.allowance = (*allowance.get()).into();
    Ok(bench)
}

fn slot<T: for<'a> ww_settings::shrink_wrap::DeserializeShrinkWrap<'a>, E>(
    record: &Record<'_>,
    slot: u8,
) -> Result<Option<T>, ReadError<E>> {
    match record.blob(slot as u32) {
        None => Ok(None),
        Some(bytes) => T::from_ww_bytes(bytes)
            .map(Some)
            .map_err(|_| ReadError::Unreadable),
    }
}

/// The boot check every guarding firmware runs first thing in `main`: read the bench record and the allowance from
/// `flash` (the board's flash, offsets as its driver counts: a `ww_settings::mapped::MappedFlash` before any
/// driver is up) where the image table's locator says, and [decide] for the board `name` with the image's own
/// [ImageId]. `Err` when the record can't be read: refuse as for [Guard::Refuse].
pub async fn boot_check<S: NorFlash>(
    table: &ImageTable<'_>,
    flash: S,
    name: &str,
    own: Option<ImageId>,
) -> Result<(Guard, BenchRecord), ReadError<S::Error>> {
    let locator = table.locator();
    if !locator.has_record() {
        return Ok((Guard::Run, BenchRecord::default()));
    }
    let mut store: Store<S, 64, 4> =
        Store::new(flash, locator.store()).map_err(|_| ReadError::Locator)?;
    let bench = read_bench(&mut store, &locator).await?;
    let known = table
        .known(name, bench.revision_str())
        .unwrap_or(ReworkSet::EMPTY);
    Ok((decide(known, bench.reworks, bench.allowance, own), bench))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{ReworkDef, ReworkTable};

    const TABLE: ReworkTable = ReworkTable::new(
        &[
            ReworkDef::new(1, "a"),
            ReworkDef::new(5, "b"),
            ReworkDef::new(8, "c"),
        ],
        &[],
    );
    const ME: ImageId = ImageId([0xa1, 2, 3, 4, 5, 6, 7, 8]);
    const OTHER: ImageId = ImageId([0xb2, 2, 3, 4, 5, 6, 7, 8]);

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    #[test]
    fn known_reworks_run() {
        let g = decide(TABLE.known(), set(&[1, 8]), StoredAllowance::NONE, Some(ME));
        assert_eq!(g, Guard::Run);
        assert!(g.runs_board());
    }

    #[test]
    fn unknown_refuses_without_allowance() {
        let g = decide(TABLE.known(), set(&[1, 9]), StoredAllowance::NONE, Some(ME));
        assert_eq!(g, Guard::Refuse { unknown: set(&[9]) });
        assert!(!g.runs_board());
    }

    #[test]
    fn allowance_must_cover_every_unknown() {
        let a = StoredAllowance::new(ME, set(&[9]));
        assert_eq!(
            decide(TABLE.known(), set(&[9]), a, Some(ME)),
            Guard::RunAllowed { unknown: set(&[9]) }
        );
        // a second unknown rework, recorded later by a newer firmware, isn't covered
        assert_eq!(
            decide(TABLE.known(), set(&[9, 10]), a, Some(ME)),
            Guard::Refuse {
                unknown: set(&[9, 10])
            }
        );
        // covering more than needed is fine
        let wide = StoredAllowance::new(ME, set(&[9, 10, 11]));
        assert!(decide(TABLE.known(), set(&[10]), wide, Some(ME)).runs_board());
    }

    #[test]
    fn allowance_for_another_image_is_void() {
        let a = StoredAllowance::new(OTHER, set(&[9]));
        assert_eq!(
            decide(TABLE.known(), set(&[9]), a, Some(ME)),
            Guard::Refuse { unknown: set(&[9]) }
        );
    }

    #[test]
    fn image_without_build_id_refuses() {
        let a = StoredAllowance::new(ME, set(&[9]));
        assert_eq!(
            decide(TABLE.known(), set(&[9]), a, None),
            Guard::Refuse { unknown: set(&[9]) }
        );
        assert_eq!(ImageId::from_build_id(&[1, 2, 3]), None);
        assert_eq!(ImageId::from_build_id(&[0; 20]), None);
        assert_eq!(
            ImageId::from_build_id(&[0xa1, 2, 3, 4, 5, 6, 7, 8, 9]),
            Some(ME)
        );
    }

    #[test]
    fn stored_round_trip() {
        let a = StoredAllowance::new(ME, set(&[9, 63]));
        assert_eq!(a.image_id(), Some(ME));
        assert_eq!(StoredAllowance::NONE.image_id(), None);
        assert_eq!(
            StoredAllowance::for_reworks(ME, [ReworkId(9), ReworkId(63)]),
            Ok(a)
        );
        assert_eq!(
            StoredAllowance::for_reworks(ME, [ReworkId(9), ReworkId(64)]),
            Err(ReworkId(64))
        );
        assert_eq!(StoredAllowance::from(AllowanceGroup::from(a)), a);
        assert_eq!(
            StoredAllowance::from(AllowanceGroup::defaults()),
            StoredAllowance::NONE
        );
    }

    use ww_settings::SettingsGroup;
}
