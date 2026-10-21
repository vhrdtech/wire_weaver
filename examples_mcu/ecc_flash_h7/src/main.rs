//! SRV-19 / chunky ECC-2 on hardware: real uncorrectable ECC errors, read through `ww_ecc_flash` in both modes.
//!
//! Programming only clears bits, so a 32-byte flash word programmed twice holds the AND of both data and of both
//! ECCs, which mostly don't match. Even words of the scratch sector are programmed once (control), odd words
//! twice. Then: zeros mode reads failed words as zeros and counts them, strict mode fails with `Error::Ecc` at the
//! same words, an erase makes everything readable again. See README.md.
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use defmt::{info, warn};
use defmt_rtt as _;
use embassy_stm32::flash::{FLASH_BASE, Flash};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use panic_probe as _;
use ww_ecc_flash::{EccSafeFlash, Error};

/// Last 128 KiB sector of the STM32H725IG (offset from FLASH_BASE), left out of `memory.x`.
const SCRATCH: u32 = 0x000E_0000;
const SECTOR: u32 = 128 * 1024;
const WORD: usize = 32;
/// Words under test: even ones programmed once, odd ones twice.
const WORDS: usize = 64;
/// Odd words whose second program is all zeros (chunky's one allowed second write).
const ZERO_SECOND: usize = 4;

/// FLASH_SR1 / FLASH_CCR1 (bank 1, the only bank of the H72x/H73x): single ECC error, corrected (bit 15).
const FLASH_SR1: *const u32 = 0x5200_2010 as *const u32;
const FLASH_CCR1: *mut u32 = 0x5200_2014 as *mut u32;
const SNECCERR: u32 = 1 << 15;

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(Default::default());
    // SAFETY: embassy's `Flash` offsets start at FLASH_BASE
    let mut flash = unsafe { EccSafeFlash::new(Flash::new_blocking(p.FLASH), FLASH_BASE as u32) };

    info!(
        "erasing the scratch sector at {:#x}",
        FLASH_BASE as u32 + SCRATCH
    );
    flash.erase(SCRATCH, SCRATCH + SECTOR).unwrap();
    let mut rng = 0x2545_F491u32;
    let mut first = [[0u8; WORD]; WORDS];
    let mut second = [[0xFFu8; WORD]; WORDS];
    for i in 0..WORDS {
        first[i] = random_word(&mut rng);
        if i % 2 == 1 {
            second[i] = if i / 2 < ZERO_SECOND {
                [0; WORD]
            } else {
                random_word(&mut rng)
            };
        }
    }
    for i in 0..WORDS {
        flash.write(word_offset(i), &first[i]).unwrap();
        if i % 2 == 1
            && let Err(e) = flash.write(word_offset(i), &second[i])
        {
            panic!(
                "the flash refused the second program of word {} ({:?}): no ECC error to read, the H7 path needs \
                 a power cut instead",
                i, e
            );
        }
    }

    // zeros mode
    let mut failed = [false; WORDS];
    let (mut corrected, mut clean) = (0, 0);
    let mut zero_second_failed = 0;
    for i in 0..WORDS {
        let before = ww_ecc_flash::ecc_errors();
        clear_sneccerr();
        let mut w = [0xAA; WORD];
        flash.read(word_offset(i), &mut w).unwrap();
        failed[i] = ww_ecc_flash::ecc_errors() != before;
        let single = sneccerr();
        // embassy's next program or erase fails while the flag is set
        clear_sneccerr();
        let expected: [u8; WORD] = core::array::from_fn(|b| first[i][b] & second[i][b]);
        if failed[i] {
            assert!(i % 2 == 1, "control word {} failed its ECC check", i);
            assert_eq!(
                w, [0; WORD],
                "word {} failed ECC but did not read as zeros",
                i
            );
            if i / 2 < ZERO_SECOND {
                zero_second_failed += 1;
            }
        } else {
            assert_eq!(
                w, expected,
                "word {} passed ECC with other data than written",
                i
            );
            if i % 2 == 1 {
                if single {
                    corrected += 1;
                } else {
                    clean += 1;
                }
            }
        }
    }
    let n_failed = failed.iter().filter(|&&f| f).count();
    info!(
        "zeros mode: {} of {} twice-programmed words failed ECC (read as zeros), {} corrected, {} read clean; \
         second program of zeros: {} of {} failed; ecc_errors() = {}",
        n_failed,
        WORDS / 2,
        corrected,
        clean,
        zero_second_failed,
        ZERO_SECOND,
        ww_ecc_flash::ecc_errors()
    );
    assert!(
        n_failed > 0,
        "no word failed its ECC check: nothing to test the strict mode with"
    );
    if zero_second_failed > 0 {
        warn!(
            "a second program of all zeros left an ECC error: chunky's zero overwrite is not safe on this H7"
        );
    }

    // strict mode
    let mut flash = unsafe { EccSafeFlash::new_strict(flash.into_inner(), FLASH_BASE as u32) };
    let first_failed = failed.iter().position(|&f| f).unwrap();
    let mut all = [0u8; WORD * WORDS];
    assert_eq!(
        flash.read(SCRATCH, &mut all),
        Err(Error::Ecc {
            offset: word_offset(first_failed)
        }),
        "strict read of the whole range"
    );
    for i in 0..WORDS {
        let mut w = [0xAA; WORD];
        // a single byte in the middle of the word is enough to hit it
        let r = flash.read(word_offset(i) + 13, &mut w[..1]);
        if failed[i] {
            let e = r.unwrap_err();
            assert!(
                e.is_ecc()
                    && e == Error::Ecc {
                        offset: word_offset(i)
                    },
                "word {}: {:?}",
                i,
                e
            );
        } else {
            r.unwrap();
            flash.read(word_offset(i), &mut w).unwrap();
            let expected: [u8; WORD] = core::array::from_fn(|b| first[i][b] & second[i][b]);
            assert_eq!(w, expected, "strict read of word {}", i);
        }
    }
    info!(
        "strict mode: Error::Ecc at the same {} words, the first at offset {:#x}",
        n_failed,
        word_offset(first_failed)
    );

    clear_sneccerr();
    flash.erase(SCRATCH, SCRATCH + SECTOR).unwrap();
    flash.read(SCRATCH, &mut all).unwrap();
    assert!(
        all.iter().all(|&b| b == 0xFF),
        "erased words read back wrong"
    );
    info!("after an erase every word reads 0xFF in strict mode");

    info!("ECC-STRICT H7 PASSED");
    loop {
        cortex_m::asm::bkpt();
    }
}

fn word_offset(i: usize) -> u32 {
    SCRATCH + (i * WORD) as u32
}

fn random_word(rng: &mut u32) -> [u8; WORD] {
    let mut w = [0; WORD];
    for chunk in w.as_chunks_mut::<4>().0 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 17;
        *rng ^= *rng << 5;
        *chunk = rng.to_le_bytes();
    }
    w
}

fn sneccerr() -> bool {
    unsafe { read_volatile(FLASH_SR1) & SNECCERR != 0 }
}

fn clear_sneccerr() {
    unsafe { write_volatile(FLASH_CCR1, SNECCERR) };
}
