//! Host tests: the flash is a buffer of this thread, its words with an uncorrectable ECC error a list, read like
//! the families read theirs (zeros and `true` for a broken word, counted).

use std::cell::RefCell;
use std::vec::Vec;

pub(crate) const BASE: u32 = 0x0800_0000;

std::thread_local! {
    static FLASH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Memory-mapped addresses of the flash words that fail their ECC check.
    static BROKEN: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn init() {}

/// The flash of this thread: `size` erased bytes, no broken words.
pub(crate) fn reset(size: usize) {
    FLASH.with_borrow_mut(|f| *f = std::vec![0xFF; size]);
    BROKEN.with_borrow_mut(|b| b.clear());
}

/// The flash word at memory-mapped `addr` fails its ECC check until it is erased.
pub(crate) fn break_word(addr: u32) {
    BROKEN.with_borrow_mut(|b| b.push(addr));
}

pub(crate) unsafe fn read_word(addr: u32, out: &mut [u8]) -> bool {
    let start = (addr - BASE) as usize;
    FLASH.with_borrow(|f| out.copy_from_slice(&f[start..start + out.len()]));
    let failed = BROKEN.with_borrow(|b| b.contains(&addr));
    if failed {
        // what the hardware gives is garbage, the families return zeros
        out.fill(0);
        crate::count_ecc_error();
    }
    failed
}

/// `NorFlash` over the same buffer, `WORD`-byte ECC words, 256-byte pages; an erase heals broken words.
pub(crate) struct SimFlash<const WORD: usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SimError;

impl embedded_storage::nor_flash::NorFlashError for SimError {
    fn kind(&self) -> embedded_storage::nor_flash::NorFlashErrorKind {
        embedded_storage::nor_flash::NorFlashErrorKind::Other
    }
}

impl<const WORD: usize> embedded_storage::nor_flash::ErrorType for SimFlash<WORD> {
    type Error = SimError;
}

impl<const WORD: usize> embedded_storage::nor_flash::ReadNorFlash for SimFlash<WORD> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, _offset: u32, _bytes: &mut [u8]) -> Result<(), SimError> {
        unreachable!("EccSafeFlash reads the memory-mapped flash itself")
    }

    fn capacity(&self) -> usize {
        FLASH.with_borrow(|f| f.len())
    }
}

impl<const WORD: usize> embedded_storage::nor_flash::NorFlash for SimFlash<WORD> {
    const WRITE_SIZE: usize = WORD;
    const ERASE_SIZE: usize = 256;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), SimError> {
        FLASH.with_borrow_mut(|f| f[from as usize..to as usize].fill(0xFF));
        BROKEN.with_borrow_mut(|b| b.retain(|&a| a < BASE + from || a >= BASE + to));
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), SimError> {
        FLASH.with_borrow_mut(|f| {
            f[offset as usize..offset as usize + bytes.len()].copy_from_slice(bytes)
        });
        Ok(())
    }
}
