//! The bench record and the allowance through a boot ROM (STD-101, feature `host`).
//!
//! A board that refuses to boot sits in its ROM bootloader, where no firmware answers. A flasher (TOOL-29) and
//! `ww info` (TOOL-30) read the store there instead: the image table's [Locator] (of the image in flash, or of the
//! image about to be written) says where it is; the host reads [Locator::store_addr] (PICOBOOT read on the RP2350,
//! DFU upload on an STM32) and gives the bytes to [read]. [set_allowance] makes a change to the allowance on those
//! bytes with the real `ww_settings` store and answers with the flash operations that make it on the device:
//! replayed through the ROM in order, a cut part way leaves what a power cut on the device would.
//!
//! ```
//! use ww_board_info::{guard::{ImageId, StoredAllowance}, image::Locator, record, table::ReworkSet};
//!
//! let locator = Locator {
//!     flash_base: 0x1000_0000, store_start: 0x3f_0000, store_len: 0x2000, page_size: 4096, word_size: 1,
//!     record_key: 1, revision_slot: 0, reworks_slot: 1, reworks_width: 4, allowance_key: 2,
//! };
//! let erased = vec![0xff; 0x2000]; // what the ROM read
//! assert_eq!(record::read(&locator, &erased).unwrap().allowance, StoredAllowance::NONE);
//!
//! let allowance = StoredAllowance::new(ImageId([1; 8]), ReworkSet(1 << 9));
//! let change = record::set_allowance(&locator, &erased, allowance).unwrap();
//! assert!(!change.ops.is_empty()); // replay these at their memory addresses through the ROM
//! assert_eq!(record::read(&locator, &change.image).unwrap().allowance, allowance);
//! ```

use std::fmt;

use embedded_storage_async::nor_flash::NorFlash;
use ww_settings::dump::{DumpError, JOB_BUF, JOB_KEYS, StoreJob, run_on_image};
use ww_settings::fake_flash::FlashOp;
use ww_settings::{Debounce, Key, Settings, Store};

use crate::guard::{
    AllowanceGroup, BenchRecord, MAX_RECORD, ReadError, StoredAllowance, read_bench,
};
use crate::image::Locator;

/// Why the record couldn't be read or changed.
#[derive(Debug)]
pub enum RecordError {
    /// The image's locator says there is no bench record.
    NoRecord,
    /// The bytes don't fit the locator (length, word or page size).
    Dump(DumpError),
    /// The record or the allowance is from a newer format, or doesn't decode.
    Unreadable,
    /// The store refused the change (full, or a record from a newer format).
    Write(String),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::NoRecord => f.write_str("the image names no bench record"),
            RecordError::Dump(e) => write!(f, "the board's settings store: {e}"),
            RecordError::Unreadable => {
                f.write_str("the board's record is from a newer format or damaged")
            }
            RecordError::Write(e) => write!(f, "the allowance couldn't be written: {e}"),
        }
    }
}

impl std::error::Error for RecordError {}

/// A change to make on the device, see [set_allowance].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Erases and writes at memory addresses ([Locator::store_addr]), in the order to replay them. Empty: the
    /// store holds this allowance already.
    pub ops: Vec<FlashOp>,
    /// The store's bytes after them.
    pub image: Vec<u8>,
}

/// Read the bench record and the allowance from the store's bytes (read at [Locator::store_addr]).
pub fn read(locator: &Locator, store: &[u8]) -> Result<BenchRecord, RecordError> {
    if !locator.has_record() {
        return Err(RecordError::NoRecord);
    }
    struct Read(Locator);
    impl StoreJob for Read {
        type Output = Result<BenchRecord, RecordError>;
        async fn run<S: NorFlash>(self, store: &mut Store<S, JOB_BUF, JOB_KEYS>) -> Self::Output {
            read_bench(store, &self.0).await.map_err(read_error)
        }
    }
    run(locator, store, Read(*locator))?.output
}

/// Store `allowance` (or none: [StoredAllowance::NONE] clears it) in the store's bytes; the operations to replay.
/// An allowance from a newer format is replaced.
pub fn set_allowance(
    locator: &Locator,
    store: &[u8],
    allowance: StoredAllowance,
) -> Result<Change, RecordError> {
    if !locator.has_record() {
        return Err(RecordError::NoRecord);
    }
    struct Set(Key, StoredAllowance);
    impl StoreJob for Set {
        type Output = Result<(), RecordError>;
        async fn run<S: NorFlash>(self, store: &mut Store<S, JOB_BUF, JOB_KEYS>) -> Self::Output {
            let mut group: Settings<AllowanceGroup, MAX_RECORD> =
                Settings::new(self.0, Debounce::default());
            group
                .load(store)
                .await
                .map_err(|e| RecordError::Write(format!("{e:?}")))?;
            if group.is_read_only() {
                group.reset_to_defaults(0);
            }
            group.set(0, |g| *g = self.1.into());
            group
                .persist(store)
                .await
                .map_err(|e| RecordError::Write(format!("{e:?}")))?;
            Ok(())
        }
    }
    let edited = run(locator, store, Set(Key(locator.allowance_key), allowance))?;
    edited.output?;
    // the store ran on flash offsets, the ROM takes memory addresses
    let at_memory = |offset: u32| offset + locator.flash_base;
    let ops = edited
        .ops
        .into_iter()
        .map(|op| match op {
            FlashOp::Erase { from, to } => FlashOp::Erase {
                from: at_memory(from),
                to: at_memory(to),
            },
            FlashOp::Write { offset, bytes } => FlashOp::Write {
                offset: at_memory(offset),
                bytes,
            },
        })
        .collect();
    Ok(Change {
        ops,
        image: edited.image,
    })
}

fn run<J: StoreJob>(
    locator: &Locator,
    store: &[u8],
    job: J,
) -> Result<ww_settings::dump::Edited<J::Output>, RecordError> {
    if store.len() != locator.store_len as usize {
        return Err(RecordError::Dump(DumpError::BadImageLen {
            len: store.len(),
            page_size: locator.page_size as usize,
        }));
    }
    run_on_image(
        locator.word_size as usize,
        locator.page_size as usize,
        locator.store_start,
        store,
        job,
    )
    .map_err(RecordError::Dump)
}

fn read_error<E>(e: ReadError<E>) -> RecordError {
    match e {
        ReadError::Locator => RecordError::NoRecord,
        ReadError::Store(_) | ReadError::Unreadable => RecordError::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReworkId;
    use crate::guard::ImageId;
    use crate::table::ReworkSet;
    use ww_settings::fake_flash::FakeFlash;

    const LOC: Locator = Locator {
        flash_base: 0x1000_0000,
        store_start: 0x3f_0000,
        store_len: 0x2000,
        page_size: 4096,
        word_size: 1,
        record_key: 1,
        revision_slot: 0,
        reworks_slot: 1,
        reworks_width: 4,
        allowance_key: 2,
    };

    /// The board's own group, as usb_io_fw keeps it.
    #[derive(ww_settings::Settings, Clone, Copy, Debug, PartialEq, Eq)]
    struct Facts {
        #[setting(slot = 0, default = 0)]
        revision: u8,
        #[setting(slot = 1, default = 0)]
        reworks: u32,
    }

    /// A store as a device left it: the record, then `allowance` if any.
    fn device_store(revision: u8, reworks: u32, allowance: Option<StoredAllowance>) -> Vec<u8> {
        let mut flash = FakeFlash::<1, 4096>::new(LOC.store_start, 2);
        embassy_futures::block_on(async {
            let mut store: Store<_, 64, 4> = Store::new(&mut flash, LOC.store()).unwrap();
            let mut facts: Settings<Facts, 64> = Settings::new(Key(1), Debounce::default());
            facts.set(0, |f| *f = Facts { revision, reworks });
            facts.persist(&mut store).await.unwrap();
            if let Some(a) = allowance {
                let mut g: Settings<AllowanceGroup, 64> =
                    Settings::new(Key(2), Debounce::default());
                g.set(0, |g| *g = a.into());
                g.persist(&mut store).await.unwrap();
            }
        });
        flash.image().to_vec()
    }

    fn set(ids: &[u16]) -> ReworkSet {
        ids.iter()
            .fold(ReworkSet::EMPTY, |s, &id| s.with(ReworkId(id), true))
    }

    #[test]
    fn reads_what_the_device_stored() {
        let a = StoredAllowance::new(ImageId([7; 8]), set(&[9]));
        let bench = read(&LOC, &device_store(b'A', 1 << 5 | 1 << 9, Some(a))).unwrap();
        assert_eq!(bench.revision, Some(b'A'));
        assert_eq!(bench.revision_str(), "A");
        assert_eq!(bench.reworks, set(&[5, 9]));
        assert_eq!(bench.allowance, a);
        // an erased store: no record
        let bench = read(&LOC, &[0xff; 0x2000]).unwrap();
        assert_eq!(bench, BenchRecord::default());
    }

    #[test]
    fn writes_replay_on_the_device() {
        let before = device_store(b'A', 1 << 9, None);
        let a = StoredAllowance::new(ImageId([7; 8]), set(&[9]));
        let change = set_allowance(&LOC, &before, a).unwrap();
        assert_eq!(read(&LOC, &change.image).unwrap().allowance, a);
        // the record is kept
        assert_eq!(read(&LOC, &change.image).unwrap().reworks, set(&[9]));

        // the operations, replayed at their memory addresses on the device's flash, give the same bytes
        let mut device =
            FakeFlash::<1, 4096>::from_image(LOC.flash_base + LOC.store_start, &before);
        embassy_futures::block_on(async {
            for op in &change.ops {
                match op {
                    FlashOp::Erase { from, to } => device.erase(*from, *to).await.unwrap(),
                    FlashOp::Write { offset, bytes } => device.write(*offset, bytes).await.unwrap(),
                }
            }
        });
        assert_eq!(device.image(), &change.image[..]);
        assert!(change.ops.iter().all(|op| match op {
            FlashOp::Erase { from, .. } => LOC.store_addr().contains(from),
            FlashOp::Write { offset, .. } => LOC.store_addr().contains(offset),
        }));

        // the same allowance again: nothing to do; clearing it: a write
        assert!(
            set_allowance(&LOC, &change.image, a)
                .unwrap()
                .ops
                .is_empty()
        );
        let cleared = set_allowance(&LOC, &change.image, StoredAllowance::NONE).unwrap();
        assert!(!cleared.ops.is_empty());
        assert_eq!(
            read(&LOC, &cleared.image).unwrap().allowance,
            StoredAllowance::NONE
        );
        // clearing where there is none: nothing
        assert!(
            set_allowance(&LOC, &before, StoredAllowance::NONE)
                .unwrap()
                .ops
                .is_empty()
        );
    }

    #[test]
    fn bytes_must_fit_the_locator() {
        assert!(matches!(
            read(&LOC, &[0xff; 4096]),
            Err(RecordError::Dump(_))
        ));
        assert!(matches!(
            read(&Locator::NONE, &[]),
            Err(RecordError::NoRecord)
        ));
    }

    #[test]
    fn boot_check_reads_the_same() {
        use crate::guard::{Guard, boot_check};
        use crate::image::ImageTable;
        use crate::table::{ReworkDef, ReworkTable};
        const B200A: ReworkTable = ReworkTable::new(&[ReworkDef::new(5, "b")], &[]);
        crate::image_reworks!(static IMAGE = LOC, [("B200", "A", B200A)]);
        let table = ImageTable::parse(IMAGE.as_bytes()).unwrap();
        let me = ImageId([3; 8]);

        let store = device_store(b'A', 1 << 5 | 1 << 9, None);
        let flash = || FakeFlash::<1, 4096>::from_image(LOC.store_start, &store);
        let (guard, bench) =
            embassy_futures::block_on(boot_check(&table, flash(), "B200", Some(me))).unwrap();
        assert_eq!(guard, Guard::Refuse { unknown: set(&[9]) });
        assert_eq!(bench.reworks, set(&[5, 9]));

        // allowed through the ROM for this image: it runs
        let allowed = set_allowance(&LOC, &store, StoredAllowance::new(me, set(&[9]))).unwrap();
        let flash = FakeFlash::<1, 4096>::from_image(LOC.store_start, &allowed.image);
        let (guard, _) =
            embassy_futures::block_on(boot_check(&table, flash, "B200", Some(me))).unwrap();
        assert_eq!(guard, Guard::RunAllowed { unknown: set(&[9]) });
    }
}
