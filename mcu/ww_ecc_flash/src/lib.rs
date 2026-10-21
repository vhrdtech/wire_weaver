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
//! A bootloader or an updater that checks images wants to know instead: [`EccSafeFlash::new_strict`] gives an
//! [`EccStrictFlash`], whose read fails with [`Error::Ecc`] at the first word that fails its ECC check
//! ([`Error::is_ecc`]), so the image is taken as unreadable rather than as one with zeros in it.
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
//! [`ecc_errors`] counts the words that failed their ECC check since boot, in both modes.
#![no_std]

#[cfg(not(any(feature = "stm32g0", feature = "stm32h7")))]
compile_error!("ww_ecc_flash: enable one MCU family feature: stm32g0 or stm32h7");
#[cfg(all(feature = "stm32g0", feature = "stm32h7"))]
compile_error!("ww_ecc_flash: enable only one MCU family feature: stm32g0 or stm32h7");

#[cfg(test)]
extern crate std;

#[cfg(feature = "stm32g0")]
#[cfg_attr(test, allow(dead_code))]
mod g0;
#[cfg(feature = "stm32h7")]
#[cfg_attr(test, allow(dead_code))]
mod h7;
// host tests: a simulated flash instead of the registers
#[cfg(test)]
mod sim;
#[cfg(test)]
mod tests;

#[cfg(all(feature = "stm32g0", not(test)))]
use g0 as family;
#[cfg(feature = "stm32g0")]
pub use g0::on_nmi;
#[cfg(all(feature = "stm32h7", not(test)))]
use h7 as family;
#[cfg(test)]
use sim as family;

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};
use embedded_storage::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

/// Largest ECC word (= flash write size) supported: 32 bytes (STM32H74x/H75x).
pub const MAX_WORD_SIZE: usize = 32;

static ECC_ERRORS: AtomicU32 = AtomicU32::new(0);

/// Flash words that failed their ECC check since boot: read as zeros by an [`EccSafeFlash`], or reported as
/// [`Error::Ecc`] by an [`EccStrictFlash`].
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

/// What a read does with a flash word that fails its ECC check: [`Zeros`] or [`Strict`]. Chosen at build time, so
/// a firmware only carries the mode it uses.
pub trait Mode: sealed::Sealed {
    #[doc(hidden)]
    const STRICT: bool;
}

/// The word reads as zeros (stores: sequential-storage, `ww_settings`).
pub enum Zeros {}

/// The read fails with [`Error::Ecc`] (bootloaders, image checks).
pub enum Strict {}

impl Mode for Zeros {
    const STRICT: bool = false;
}

impl Mode for Strict {
    const STRICT: bool = true;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Zeros {}
    impl Sealed for super::Strict {}
}

/// [`NorFlash`] that reads a flash word with an uncorrectable ECC error without a fault: as zeros ([`Zeros`], made
/// by [`new`](EccSafeFlash::new)), or as [`Error::Ecc`] ([`Strict`], made by
/// [`new_strict`](EccSafeFlash::new_strict)). See the crate docs.
pub struct EccSafeFlash<F, M: Mode = Zeros> {
    inner: F,
    base: u32,
    _mode: PhantomData<M>,
}

/// [`EccSafeFlash`] whose reads fail with [`Error::Ecc`] at a flash word that fails its ECC check.
pub type EccStrictFlash<F> = EccSafeFlash<F, Strict>;

impl<F: NorFlash> EccSafeFlash<F, Zeros> {
    /// Wraps `inner`, whose offset 0 is the memory-mapped flash address `base`
    /// (`embassy_stm32::flash::FLASH_BASE` for embassy's `Flash`, the region's start for one of its regions).
    /// A flash word that fails its ECC check reads as zeros.
    ///
    /// The ECC word is `F::WRITE_SIZE` bytes (8 on STM32G0, 32 or 16 on STM32H7).
    ///
    /// # Safety
    ///
    /// `base..base + inner.capacity()` must be the memory-mapped flash `inner` writes to: reads go to these
    /// addresses directly.
    pub unsafe fn new(inner: F, base: u32) -> Self {
        unsafe { Self::with_mode(inner, base) }
    }
}

impl<F: NorFlash> EccSafeFlash<F, Strict> {
    /// Like [`new`](EccSafeFlash::new), but a read fails with [`Error::Ecc`] at the first flash word that fails its
    /// ECC check, so a bootloader or an image check can tell "unreadable" from data.
    ///
    /// # Safety
    ///
    /// As for [`new`](EccSafeFlash::new).
    pub unsafe fn new_strict(inner: F, base: u32) -> Self {
        unsafe { Self::with_mode(inner, base) }
    }
}

impl<F: NorFlash, M: Mode> EccSafeFlash<F, M> {
    unsafe fn with_mode(inner: F, base: u32) -> Self {
        const {
            assert!(
                F::WRITE_SIZE.is_power_of_two()
                    && F::WRITE_SIZE >= 4
                    && F::WRITE_SIZE <= MAX_WORD_SIZE,
                "WRITE_SIZE of the inner flash must be its ECC word: 4..=32 bytes, a power of two"
            )
        };
        family::init();
        Self {
            inner,
            base,
            _mode: PhantomData,
        }
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
            let failed =
                unsafe { family::read_word(self.base + word_start as u32, &mut word[..word_size]) };
            if M::STRICT && failed {
                return Err(Error::Ecc {
                    offset: word_start as u32,
                });
            }
            let from = pos - word_start;
            let n = (word_size - from).min(end - pos);
            let out = pos - offset as usize;
            bytes[out..out + n].copy_from_slice(&word[from..from + n]);
            pos += n;
        }
        Ok(())
    }
}

/// [`EccSafeFlash`] error: the inner flash's, a read outside the flash, or (only from an [`EccStrictFlash`]) a
/// flash word that failed its ECC check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<E> {
    Flash(E),
    OutOfBounds,
    /// The flash word at `offset` (offset of the inner flash, aligned to the ECC word) has an uncorrectable ECC
    /// error: its data is unknown, the flash itself still works (erase it to use it again). Bytes of the read
    /// before that word are filled in, the rest of the buffer is not.
    Ecc {
        offset: u32,
    },
}

impl<E> Error<E> {
    /// True for [`Error::Ecc`]: the read hit a flash word with an uncorrectable ECC error.
    pub fn is_ecc(&self) -> bool {
        matches!(self, Error::Ecc { .. })
    }
}

impl<E: NorFlashError> NorFlashError for Error<E> {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Error::Flash(e) => e.kind(),
            Error::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Error::Ecc { .. } => NorFlashErrorKind::Other,
        }
    }
}

impl<F: NorFlash, M: Mode> ErrorType for EccSafeFlash<F, M> {
    type Error = Error<F::Error>;
}

impl<F: NorFlash, M: Mode> ReadNorFlash for EccSafeFlash<F, M> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.read_blocking(offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

impl<F: NorFlash, M: Mode> NorFlash for EccSafeFlash<F, M> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.inner.erase(from, to).map_err(Error::Flash)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.write(offset, bytes).map_err(Error::Flash)
    }
}

impl<F: NorFlash, M: Mode> embedded_storage_async::nor_flash::ReadNorFlash for EccSafeFlash<F, M> {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.read_blocking(offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

impl<F: NorFlash, M: Mode> embedded_storage_async::nor_flash::NorFlash for EccSafeFlash<F, M> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.inner.erase(from, to).map_err(Error::Flash)
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.inner.write(offset, bytes).map_err(Error::Flash)
    }
}
