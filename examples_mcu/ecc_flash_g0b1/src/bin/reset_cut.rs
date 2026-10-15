//! Does a system reset cut a flash operation? Alternates, one per boot: erase a page or program it whole, with a
//! SysTick reset (SYSRESETREQ) at a random point of the operation. Each boot reports what the cut left: erased,
//! programmed and other words, and the words that failed their ECC check (read through `EccSafeFlash`).
#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use cortex_m::peripheral::SCB;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::{entry, exception};
use defmt::info;
use defmt_rtt as _;
use embassy_stm32::flash::{FLASH_BASE, Flash};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use panic_probe as _;
use ww_ecc_flash::EccSafeFlash;

const PAGE: u32 = 0x0007_4000;
const PAGE_SIZE: usize = 2048;
const BOOTS: u32 = 200;
const PATTERN: u32 = 0x5A5A_0000;
const MAGIC: u32 = 0xC07_C07;

#[repr(C)]
struct State {
    magic: u32,
    boots: u32,
    rng: u32,
    /// operation cut at the last reset: 0 none, 1 erase, 2 program
    op: u32,
    delay_us: u32,
    partial_erases: u32,
    partial_programs: u32,
    ecc_boots: u32,
    /// words neither erased nor the pattern, over all boots
    other_words: u32,
    /// erases cut before the 22 ms they take, and how many of those still left the page fully erased
    early_erase_cuts: u32,
    early_erase_cuts_erased: u32,
}

#[unsafe(link_section = ".uninit.STATE")]
static mut STATE: MaybeUninit<State> = MaybeUninit::uninit();

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(Default::default());
    let mut cp = cortex_m::Peripherals::take().unwrap();
    // SAFETY: embassy's `Flash` offsets start at FLASH_BASE
    let mut flash = unsafe { EccSafeFlash::new(Flash::new_blocking(p.FLASH), FLASH_BASE as u32) };
    // SAFETY: only main touches it, all fields u32
    let s = unsafe { (*addr_of_mut!(STATE)).assume_init_mut() };
    if s.magic != MAGIC {
        *s = State {
            magic: MAGIC,
            boots: 0,
            rng: 0x2545_F491,
            op: 0,
            delay_us: 0,
            partial_erases: 0,
            partial_programs: 0,
            ecc_boots: 0,
            other_words: 0,
            early_erase_cuts: 0,
            early_erase_cuts_erased: 0,
        };
        flash.erase(PAGE, PAGE + PAGE_SIZE as u32).unwrap();
    }
    s.boots += 1;

    let mut page = [0u8; PAGE_SIZE];
    flash.read(PAGE, &mut page).unwrap();
    let (mut erased, mut programmed, mut other) = (0, 0, 0);
    for (i, w) in page.as_chunks::<8>().0.iter().enumerate() {
        let w = u64::from_le_bytes(*w);
        let expected = (PATTERN | i as u32) as u64 | ((PATTERN | i as u32) as u64) << 32;
        if w == u64::MAX {
            erased += 1;
        } else if w == expected {
            programmed += 1;
        } else {
            other += 1;
            s.other_words += 1;
        }
    }
    let ecc = ww_ecc_flash::ecc_errors();
    if ecc > 0 {
        s.ecc_boots += 1;
    }
    let partial = erased != 256 && programmed != 256;
    if partial && s.op == 1 {
        s.partial_erases += 1;
    }
    if partial && s.op == 2 {
        s.partial_programs += 1;
    }
    if s.op == 1 && s.delay_us < 20_000 {
        s.early_erase_cuts += 1;
        if erased == 256 {
            s.early_erase_cuts_erased += 1;
        }
    }
    info!(
        "boot {}: after {} cut at {} us: erased {} programmed {} other {} ECC {} | partial erases {} programs {}, boots with ECC {}",
        s.boots,
        ["-", "erase", "program"][s.op as usize % 3],
        s.delay_us,
        erased,
        programmed,
        other,
        ecc,
        s.partial_erases,
        s.partial_programs,
        s.ecc_boots
    );
    cortex_m::asm::delay(16 * 50_000); // 50 ms for the host to read RTT before the next reset
    if s.boots >= BOOTS {
        info!(
            "done: erases cut before 20 ms {}, of them left fully erased {}; words neither erased nor written {}",
            s.early_erase_cuts, s.early_erase_cuts_erased, s.other_words
        );
        s.magic = 0;
        cortex_m::asm::delay(16_000_000); // ~1 s for the host to read RTT
        loop {
            cortex_m::asm::bkpt();
        }
    }

    // erase when anything is programmed, else program the whole page; cut at a random time
    let program = erased == 256;
    s.rng ^= s.rng << 13;
    s.rng ^= s.rng >> 17;
    s.rng ^= s.rng << 5;
    // a page erase takes ~22 ms, programming 256 double words ~256 x 85 us
    s.delay_us = s.rng % 25_000 + 1;
    s.op = if program { 2 } else { 1 };
    cp.SYST.set_clock_source(SystClkSource::Core);
    cp.SYST.set_reload(s.delay_us * 16);
    cp.SYST.clear_current();
    cp.SYST.enable_interrupt();
    cp.SYST.enable_counter();
    if program {
        let mut data = [0u8; PAGE_SIZE];
        for (i, w) in data.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            let v = PATTERN | i as u32;
            w[..4].copy_from_slice(&v.to_le_bytes());
            w[4..].copy_from_slice(&v.to_le_bytes());
        }
        flash.write(PAGE, &data).unwrap();
    } else {
        flash.erase(PAGE, PAGE + PAGE_SIZE as u32).unwrap();
    }
    s.op = 0;
    // not cut: SysTick resets after the operation
    loop {}
}

#[exception]
fn SysTick() {
    SCB::sys_reset();
}
