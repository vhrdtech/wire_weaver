//! STM32G0: a double ECC error raises an NMI with `FLASH_ECCR.ECCD` (bank 1) or `FLASH_ECCR2.ECCD` (bank 2) set
//! and the double-word offset of the failed word in `ADDR_ECC`. The load itself completes with garbage data.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering, compiler_fence};

const FLASH_ECCR: *mut u32 = 0x4002_2018 as *mut u32;
const FLASH_ECCR2: *mut u32 = 0x4002_201C as *mut u32;
/// Flash size in KiB (device electronic signature).
const FLASHSIZE: *const u16 = 0x1FFF_75E0 as *const u16;
const FLASH_START: u32 = 0x0800_0000;

const ECCR_ADDR_ECC: u32 = 0xFFFF;
const ECCR_SYSF_ECC: u32 = 1 << 20;
const ECCR_ECCCIE: u32 = 1 << 24;
const ECCR_ECCD: u32 = 1 << 31;

/// Address of the double word being read by `read_word`, 0 when none.
static READING: AtomicU32 = AtomicU32::new(0);
/// Set by `on_nmi` when the double word at `READING` failed its ECC check.
static FAILED: AtomicBool = AtomicBool::new(false);

pub(crate) fn init() {}

/// Reads one 8-byte flash word at `addr`; zeros and `true` when it fails its ECC check.
///
/// # Safety
///
/// `addr` is an 8-byte aligned, memory-mapped flash address.
pub(crate) unsafe fn read_word(addr: u32, out: &mut [u8]) -> bool {
    cortex_m::interrupt::free(|_| {
        FAILED.store(false, Ordering::Relaxed);
        READING.store(addr, Ordering::Relaxed);
        compiler_fence(Ordering::SeqCst);
        for (i, chunk) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *chunk = unsafe { read_volatile((addr as *const u32).add(i)) }.to_le_bytes();
        }
        // the NMI of a failed load is taken before the next instruction completes
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        compiler_fence(Ordering::SeqCst);
        READING.store(0, Ordering::Relaxed);
        let failed = FAILED.load(Ordering::Relaxed);
        if failed {
            out.fill(0);
            crate::count_ecc_error();
        }
        failed
    })
}

/// Call first thing from the NMI handler. Returns `true` when the NMI was a double ECC error of the flash word an
/// [`EccSafeFlash`](crate::EccSafeFlash) is reading right now: the error flag is cleared, the word reads as
/// zeros, the handler returns. `false` for any other NMI (nothing is cleared): the application handles it.
pub fn on_nmi() -> bool {
    let reading = READING.load(Ordering::Relaxed);
    if reading == 0 {
        return false;
    }
    // bank 2 (dual-bank parts) starts in the middle of the flash
    let bank2_start = FLASH_START + unsafe { read_volatile(FLASHSIZE) } as u32 * 1024 / 2;
    for (eccr, bank_start) in [(FLASH_ECCR, FLASH_START), (FLASH_ECCR2, bank2_start)] {
        let v = unsafe { read_volatile(eccr) };
        if v & ECCR_ECCD == 0 || v & ECCR_SYSF_ECC != 0 {
            continue;
        }
        if bank_start + (v & ECCR_ADDR_ECC) * 8 == reading {
            // ECCD and ECCC are write-1-to-clear: clear ECCD only, keep ECCCIE
            unsafe { write_volatile(eccr, (v & ECCR_ECCCIE) | ECCR_ECCD) };
            FAILED.store(true, Ordering::Relaxed);
            return true;
        }
    }
    false
}

#[cfg(feature = "nmi-handler")]
#[cortex_m_rt::exception]
unsafe fn NonMaskableInt() {
    if !on_nmi() {
        panic!("NMI, not a flash ECC error of an EccSafeFlash read");
    }
}
