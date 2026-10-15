//! ECC-safe STM32 flash for stores (wear-levelled maps, settings, logs) on top of any [`NorFlash`], e.g.
//! embassy-stm32's `Flash<'_, Blocking>`.
//!
//! A power cut while a flash word is programmed or its page erased can leave the word with an uncorrectable ECC
//! error. Reading it then does not return data: on STM32G0 the flash raises an NMI (`FLASH_ECCR.ECCD`), whose
//! default handler hangs, on STM32H7 the read ends in a bus error (`FLASH_SR.DBECCERR`). A store that reads its
//! pages at boot would boot-loop on such a word. [`EccSafeFlash`] reads the flash itself, one ECC word at a time,
//! and returns zeros for a word that fails its ECC check. Stores built for power cuts already handle a word of
//! zeros: sequential-storage sees a header or data that fails its CRC and skips it, a page marker of zeros is a
//! legal outcome of a cut marker write.
//!
//! Writes and erases go to the inner flash unchanged. [`EccSafeFlash`] never implements `MultiwriteNorFlash`:
//! programming a word twice is not allowed with ECC (the second write changes the data but not the ECC bits).
//!
//! # STM32G0
//!
//! Feature `stm32g0`. The application's NMI handler calls [`on_nmi`] first:
//!
//! ```ignore
//! #[cortex_m_rt::exception]
//! unsafe fn NonMaskableInt() {
//!     if ww_ecc_flash::on_nmi() {
//!         return; // a word read through EccSafeFlash failed its ECC check, it reads as zeros
//!     }
//!     // any other NMI: the application's own handling
//! }
//! ```
//!
//! or enables feature `nmi-handler`, which defines that handler and panics on any other NMI. [`on_nmi`] only
//! takes an ECC error at the address [`EccSafeFlash`] is reading right then; an ECC error anywhere else (code,
//! constants, another driver's read) is left to the application.
//!
//! # STM32H7
//!
//! Feature `stm32h7`. No handler needed: each flash word is read with all exceptions masked (`FAULTMASK`) and
//! `SCB.CCR.BFHFNMIGN` set, so the bus error is ignored, then `FLASH_SR.DBECCERR` of the word's bank is checked and
//! cleared. With the D-cache on, the lines of the word are invalidated before the read. Keep the store region out of
//! the D-cache (MPU, non-cacheable) anyway: a cache linefill may report its bus error imprecisely, and an imprecise
//! bus error is not ignored.
//!
//! # Usage
//!
//! ```ignore
//! let flash = embassy_stm32::flash::Flash::new_blocking(p.FLASH);
//! // offsets of `flash` start at the beginning of the flash
//! let mut flash = unsafe { ww_ecc_flash::EccSafeFlash::new(flash, embassy_stm32::flash::FLASH_BASE as u32) };
//! // blocking and async (embedded-storage-async) NorFlash, e.g. for sequential-storage
//! ```
//!
//! [`ecc_errors`] counts the words read as zeros since boot.
#![no_std]

#[cfg(not(any(feature = "stm32g0", feature = "stm32h7")))]
compile_error!("ww_ecc_flash: enable one MCU family feature: stm32g0 or stm32h7");
#[cfg(all(feature = "stm32g0", feature = "stm32h7"))]
compile_error!("ww_ecc_flash: enable only one MCU family feature: stm32g0 or stm32h7");

#[cfg(feature = "stm32g0")]
mod g0;
#[cfg(feature = "stm32h7")]
mod h7;

#[cfg(feature = "stm32g0")]
use g0 as family;
#[cfg(feature = "stm32g0")]
pub use g0::on_nmi;
#[cfg(feature = "stm32h7")]
use h7 as family;

use core::sync::atomic::{AtomicU32, Ordering};
use embedded_storage::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

/// Largest ECC word (= flash write size) supported: 32 bytes (STM32H74x/H75x).
pub const MAX_WORD_SIZE: usize = 32;

static ECC_ERRORS: AtomicU32 = AtomicU32::new(0);

/// Flash words read as zeros since boot because their ECC check failed.
pub fn ecc_errors() -> u32 {
    ECC_ERRORS.load(Ordering::Relaxed)
}

fn count_ecc_error() {
    // load + store: thumbv6m has no atomic read-modify-write; called with interrupts masked
    ECC_ERRORS.store(
        ECC_ERRORS.load(Ordering::Relaxed).wrapping_add(1),
        Ordering::Relaxed,
    );
}

/// [`NorFlash`] whose reads return zeros for a flash word with an uncorrectable ECC error, see the crate docs.
pub struct EccSafeFlash<F> {
    inner: F,
    base: u32,
}

impl<F: NorFlash> EccSafeFlash<F> {
    /// Wraps `inner`, whose offset 0 is the memory-mapped flash address `base`
    /// (`embassy_stm32::flash::FLASH_BASE` for embassy's `Flash`, the region's start for one of its regions).
    ///
    /// The ECC word is `F::WRITE_SIZE` bytes (8 on STM32G0, 32 or 16 on STM32H7).
    ///
    /// # Safety
    ///
    /// `base..base + inner.capacity()` must be the memory-mapped flash `inner` writes to: reads go to these
    /// addresses directly.
    pub unsafe fn new(inner: F, base: u32) -> Self {
        const {
            assert!(
                F::WRITE_SIZE.is_power_of_two()
                    && F::WRITE_SIZE >= 4
                    && F::WRITE_SIZE <= MAX_WORD_SIZE,
                "WRITE_SIZE of the inner flash must be its ECC word: 4..=32 bytes, a power of two"
            )
        };
        family::init();
        Self { inner, base }
    }

    /// The wrapped flash.
    pub fn inner(&mut self) -> &mut F {
        &mut self.inner
    }

    /// Gives back the wrapped flash.
    pub fn into_inner(self) -> F {
        self.inner
    }

    fn read_blocking(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error<F::Error>> {
        let end = offset as usize + bytes.len();
        if end > self.inner.capacity() {
            return Err(Error::OutOfBounds);
        }
        let word_size = F::WRITE_SIZE;
        let mut word = [0u8; MAX_WORD_SIZE];
        let mut pos = offset as usize;
        while pos < end {
            let word_start = pos & !(word_size - 1);
            // SAFETY: inside the flash, aligned to the ECC word (see `new`)
            unsafe { family::read_word(self.base + word_start as u32, &mut word[..word_size]) };
            let from = pos - word_start;
            let n = (word_size - from).min(end - pos);
            let out = pos - offset as usize;
            bytes[out..out + n].copy_from_slice(&word[from..from + n]);
            pos += n;
        }
        Ok(())
    }
}

/// [`EccSafeFlash`] error: the inner flash's, or a read outside the flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    Flash(E),
    OutOfBounds,
}

impl<E: NorFlashError> NorFlashError for Error<E> {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Error::Flash(e) => e.kind(),
            Error::OutOfBounds => NorFlashErrorKind::OutOfBounds,
        }
    }
}

impl<F: NorFlash> ErrorType for EccSafeFlash<F> {
    type Error = Error<F::Error>;
}

impl<F: NorFlash> ReadNorFlash for EccSafeFlash<F> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.read_blocking(offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

impl<F: NorFlash> NorFlash for EccSafeFlash<F> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.inner.erase(from, to).map_err(Error::Flash)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.write(offset, bytes).map_err(Error::Flash)
    }
}

impl<F: NorFlash> embedded_storage_async::nor_flash::ReadNorFlash for EccSafeFlash<F> {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.read_blocking(offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

impl<F: NorFlash> embedded_storage_async::nor_flash::NorFlash for EccSafeFlash<F> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.inner.erase(from, to).map_err(Error::Flash)
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.write(offset, bytes).map_err(Error::Flash)
    }
}
