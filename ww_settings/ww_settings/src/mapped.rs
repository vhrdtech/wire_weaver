//! [MappedFlash]: a [Store](crate::Store) read straight from memory-mapped flash (XIP on the RP2350, the internal
//! flash of an STM32), before any flash driver or HAL is up. Reads are volatile loads; writes and erases are
//! refused. What a firmware's first lines use to read a record at boot (the rework guard, STD-101).

use embedded_storage_async::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

/// Why a [MappedFlash] operation failed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum MappedError {
    /// A write or an erase: the flash is read-only here.
    ReadOnly,
    /// Outside the mapped flash.
    OutOfBounds,
}

impl NorFlashError for MappedError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            MappedError::ReadOnly => NorFlashErrorKind::Other,
            MappedError::OutOfBounds => NorFlashErrorKind::OutOfBounds,
        }
    }
}

/// Flash mapped into memory at `base`: offset 0 of the flash (as the board's flash driver counts, the offsets a
/// [Store](crate::Store) range uses) is at `base`. `WORD` and `PAGE` are the write and erase sizes of the real flash,
/// so a store over it lays out its pages as the driver's does.
pub struct MappedFlash<const WORD: usize, const PAGE: usize> {
    base: usize,
    len: usize,
}

impl<const WORD: usize, const PAGE: usize> MappedFlash<WORD, PAGE> {
    /// # Safety
    ///
    /// `base..base + len` is readable memory for as long as this lives: mapped flash that nothing erases or
    /// programs meanwhile.
    pub const unsafe fn new(base: usize, len: usize) -> Self {
        MappedFlash { base, len }
    }
}

impl<const WORD: usize, const PAGE: usize> ErrorType for MappedFlash<WORD, PAGE> {
    type Error = MappedError;
}

impl<const WORD: usize, const PAGE: usize> ReadNorFlash for MappedFlash<WORD, PAGE> {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        let end = start
            .checked_add(bytes.len())
            .ok_or(MappedError::OutOfBounds)?;
        if end > self.len {
            return Err(MappedError::OutOfBounds);
        }
        for (i, b) in bytes.iter_mut().enumerate() {
            // Safety: inside base..base + len, readable by the contract of `new`
            *b = unsafe { core::ptr::read_volatile((self.base + start + i) as *const u8) };
        }
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.len
    }
}

impl<const WORD: usize, const PAGE: usize> NorFlash for MappedFlash<WORD, PAGE> {
    const WRITE_SIZE: usize = WORD;
    const ERASE_SIZE: usize = PAGE;

    async fn write(&mut self, _offset: u32, _bytes: &[u8]) -> Result<(), Self::Error> {
        Err(MappedError::ReadOnly)
    }

    async fn erase(&mut self, _from: u32, _to: u32) -> Result<(), Self::Error> {
        Err(MappedError::ReadOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Debounce, Key, Settings, Store};

    #[derive(crate::Settings, Clone, Copy, Debug, PartialEq, Eq)]
    struct Facts {
        #[setting(slot = 0, default = 0)]
        revision: u8,
        #[setting(slot = 1, default = 0)]
        reworks: u32,
    }

    #[test]
    fn reads_a_store_written_elsewhere() {
        use crate::fake_flash::FakeFlash;
        let mut fake = FakeFlash::<4, 4096>::new(0, 2);
        embassy_futures::block_on(async {
            let mut store: Store<_, 64, 2> = Store::new(&mut fake, 0..8192).unwrap();
            let mut facts: Settings<Facts, 64> = Settings::new(Key::FACTORY, Debounce::default());
            facts.set(0, |f| {
                f.revision = b'A';
                f.reworks = 0b1010;
            });
            facts.persist(&mut store).await.unwrap();
        });
        let image = fake.image().to_vec();

        let mut mapped =
            unsafe { MappedFlash::<4, 4096>::new(image.as_ptr() as usize, image.len()) };
        embassy_futures::block_on(async {
            let mut store: Store<_, 64, 2> = Store::new(&mut mapped, 0..8192).unwrap();
            let mut facts: Settings<Facts, 64> = Settings::new(Key::FACTORY, Debounce::default());
            assert!(facts.load(&mut store).await.unwrap().found);
            assert_eq!(
                *facts.get(),
                Facts {
                    revision: b'A',
                    reworks: 0b1010
                }
            );
            assert!(matches!(
                store.store(Key::USER, &[1, 2, 3]).await,
                Err(crate::StoreError::Flash(MappedError::ReadOnly))
            ));
        });
    }
}
