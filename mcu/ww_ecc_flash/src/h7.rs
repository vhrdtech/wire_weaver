//! STM32H7: a double ECC error sets `FLASH_SR.DBECCERR` of the bank and ends the AXI read with a bus error.
//! With FAULTMASK set (execution priority -1) and `SCB.CCR.BFHFNMIGN`, a precise data bus error is ignored: the
//! load completes with garbage data, the flag tells which word failed.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, compiler_fence};

/// FLASH_SR1; FLASH_SR2 is 0x100 above (bank 2 from 0x0810_0000)
const FLASH_SR1: u32 = 0x5200_2010;
/// FLASH_CCR1; FLASH_CCR2 is 0x100 above
const FLASH_CCR1: u32 = 0x5200_2014;
const BANK2_START: u32 = 0x0810_0000;
const SR_DBECCERR: u32 = 1 << 26;
const CCR_CLR_DBECCERR: u32 = 1 << 26;

const SCB_CCR: *mut u32 = 0xE000_ED14 as *mut u32;
const SCB_CCR_BFHFNMIGN: u32 = 1 << 8;
/// CFSR; its BFSR byte (bits 8..16) is write-1-to-clear
const SCB_CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
const CFSR_BFSR: u32 = 0xFF00;
/// D-cache invalidate by address
const SCB_DCIMVAC: *mut u32 = 0xE000_EF5C as *mut u32;
const DCACHE_LINE: u32 = 32;

pub(crate) fn init() {}

/// Reads one flash word (16 or 32 bytes) at `addr`; zeros and `true` when it fails its ECC check.
///
/// # Safety
///
/// `addr` is a memory-mapped flash address aligned to the flash word.
pub(crate) unsafe fn read_word(addr: u32, out: &mut [u8]) -> bool {
    let (sr, ccr) = if addr >= BANK2_START {
        (FLASH_SR1 + 0x100, FLASH_CCR1 + 0x100)
    } else {
        (FLASH_SR1, FLASH_CCR1)
    };
    let was_masked = cortex_m::register::faultmask::read().is_active();
    unsafe {
        faultmask_set();
        let scb_ccr = read_volatile(SCB_CCR);
        write_volatile(SCB_CCR, scb_ccr | SCB_CCR_BFHFNMIGN);
        // fresh data after a program or erase, and no hit on a cached copy of a word that failed
        let mut line = addr & !(DCACHE_LINE - 1);
        while line < addr + out.len() as u32 {
            write_volatile(SCB_DCIMVAC, line);
            line += DCACHE_LINE;
        }
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        compiler_fence(Ordering::SeqCst);
        for (i, chunk) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *chunk = read_volatile((addr as *const u32).add(i)).to_le_bytes();
        }
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        compiler_fence(Ordering::SeqCst);
        let failed = read_volatile(sr as *const u32) & SR_DBECCERR != 0;
        if failed {
            write_volatile(ccr as *mut u32, CCR_CLR_DBECCERR);
            // the ignored bus error left PRECISERR / BFARVALID behind
            let bfsr = read_volatile(SCB_CFSR) & CFSR_BFSR;
            write_volatile(SCB_CFSR, bfsr);
            out.fill(0);
            crate::count_ecc_error();
        }
        write_volatile(SCB_CCR, scb_ccr);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        if !was_masked {
            faultmask_clear();
        }
        failed
    }
}

#[cfg(target_arch = "arm")]
unsafe fn faultmask_set() {
    unsafe { core::arch::asm!("cpsid f", options(nostack, preserves_flags)) };
}

#[cfg(target_arch = "arm")]
unsafe fn faultmask_clear() {
    unsafe { core::arch::asm!("cpsie f", options(nostack, preserves_flags)) };
}

// host builds (cargo check of the mcu workspace): like cortex-m's own register access off target
#[cfg(not(target_arch = "arm"))]
unsafe fn faultmask_set() {
    unimplemented!()
}

#[cfg(not(target_arch = "arm"))]
unsafe fn faultmask_clear() {
    unimplemented!()
}
