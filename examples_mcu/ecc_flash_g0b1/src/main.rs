//! TEST-21: a sequential-storage map on `ww_ecc_flash::EccSafeFlash` survives resets in the middle of its flash
//! programs and page erases.
//!
//! Every boot loads all keys and checks them against what the previous boots committed (kept in RAM that a reset
//! doesn't clear), then arms SysTick for a random delay and stores new values until SysTick resets the chip, often
//! in the middle of a double-word program or a page erase. A cut write leaves words with ECC errors: they read as
//! zeros through `EccSafeFlash`, sequential-storage skips them. After `BOOTS` boots the summary is printed and the
//! core halts (probe-rs run exits). Any load error, wrong value or lost committed value stops the test right away.
#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::ops::Range;
use core::ptr::addr_of_mut;
use cortex_m::peripheral::SCB;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::{entry, exception};
use defmt::{Debug2Format, error, info, warn};
use defmt_rtt as _;
use embassy_futures::block_on;
use embassy_stm32::flash::{Blocking, FLASH_BASE, Flash};
use embedded_storage::nor_flash::{ErrorType, NorFlash};
use panic_probe as _;
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage};
use ww_ecc_flash::EccSafeFlash;

/// Boots to run before the summary.
const BOOTS: u32 = 3000;
/// Store: 4 pages of 2 KiB in bank 2 (offsets from the start of the flash). The code runs from bank 1, so it keeps
/// running (and SysTick fires) while bank 2 programs or erases.
const STORE: Range<u32> = 0x0007_0000..0x0007_2000;
/// Value length per key: u32 sequence number + payload. The big one fills a page in a few stores, so page erases
/// (migrations) happen every few boots.
const VALUE_LEN: [usize; 3] = [4 + 4, 4 + 60, 4 + 300];
const KEYS: usize = VALUE_LEN.len();
/// SysTick reset delay after the load: uniform in 1..=MAX_DELAY_CYCLES (HSI16: 16 cycles per µs, 150 ms). Long
/// enough for a store with a page migration (~25 ms erase + copying) to complete now and then: with a shorter one
/// the store can be cut in the same migration every boot and never progress.
const MAX_DELAY_CYCLES: u32 = 16 * 150_000;
const MAGIC: u32 = 0xECC5_AFE1;
const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, defmt::Format)]
#[repr(u32)]
enum Phase {
    Idle = 0,
    Read = 1,
    Program = 2,
    Erase = 3,
}

/// Kept across resets (`.uninit` RAM): what the previous boots did and committed.
#[repr(C)]
struct Stats {
    magic: u32,
    boots: u32,
    /// flash operation running when the last reset hit
    phase: u32,
    resets_in: [u32; 4],
    stores_done: u32,
    /// key being stored when the last reset hit, `NONE` between stores
    inflight: u32,
    /// sequence number committed per key (store_item returned Ok, or the load found the in-flight one)
    committed: [u32; KEYS],
    /// the load found the value whose store was cut by the reset
    inflight_landed: u32,
    inflight_lost: u32,
    ecc_words: u32,
    boots_with_ecc: u32,
    max_ecc_words: u32,
    rng: u32,
}

#[unsafe(link_section = ".uninit.STATS")]
static mut STATS: MaybeUninit<Stats> = MaybeUninit::uninit();

fn stats() -> &'static mut Stats {
    // SAFETY: single core, only touched from main (SysTick only resets); every field is a u32, any bit pattern is
    // valid, the magic tells initialized from power-on garbage
    unsafe { (*addr_of_mut!(STATS)).assume_init_mut() }
}

/// Records the flash operation in progress, so the next boot knows what the reset cut.
struct PhaseFlash<F>(F);

impl<F: NorFlash> PhaseFlash<F> {
    fn with<R>(&mut self, phase: Phase, f: impl FnOnce(&mut F) -> R) -> R {
        stats().phase = phase as u32;
        let r = f(&mut self.0);
        stats().phase = Phase::Idle as u32;
        r
    }
}

impl<F: NorFlash> ErrorType for PhaseFlash<F> {
    type Error = F::Error;
}

impl<F: NorFlash> embedded_storage_async::nor_flash::ReadNorFlash for PhaseFlash<F> {
    const READ_SIZE: usize = F::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.with(Phase::Read, |f| f.read(offset, bytes))
    }

    fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl<F: NorFlash> embedded_storage_async::nor_flash::NorFlash for PhaseFlash<F> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.with(Phase::Erase, |f| f.erase(from, to))
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.with(Phase::Program, |f| f.write(offset, bytes))
    }
}

type Store = MapStorage<
    u8,
    PhaseFlash<EccSafeFlash<Flash<'static, Blocking>>>,
    Cache<Uncached, Uncached, Uncached, u8>,
>;

fn value(key: usize, seq: u32, out: &mut [u8]) -> &[u8] {
    let out = &mut out[..VALUE_LEN[key]];
    out[..4].copy_from_slice(&seq.to_le_bytes());
    for (i, b) in out[4..].iter_mut().enumerate() {
        *b = (seq as u8)
            .wrapping_mul(31)
            .wrapping_add((i * 7 + key) as u8);
    }
    out
}

fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn fail(what: &str) -> ! {
    error!("TEST-21 FAILED at boot {}: {}", stats().boots, what);
    summary();
    cortex_m::asm::delay(16_000_000); // ~1 s for the host to read RTT
    loop {
        cortex_m::asm::bkpt();
    }
}

fn summary() {
    let s = stats();
    info!(
        "boots {}, stores {}, resets in idle/read/program/erase {}, cut stores found landed {} / lost {}, \
         words read as zeros (ECC) {} in {} boots (max {} in one)",
        s.boots,
        s.stores_done,
        s.resets_in,
        s.inflight_landed,
        s.inflight_lost,
        s.ecc_words,
        s.boots_with_ecc,
        s.max_ecc_words,
    );
}

#[entry]
fn main() -> ! {
    let p = embassy_stm32::init(Default::default());
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let flash = Flash::new_blocking(p.FLASH);
    // SAFETY: embassy's `Flash` offsets start at FLASH_BASE
    let flash = unsafe { EccSafeFlash::new(flash, FLASH_BASE as u32) };
    let mut store: Store = MapStorage::new(
        PhaseFlash(flash),
        const { MapConfig::new(STORE) },
        Cache::new_uncached(),
    );
    let mut buf = [0u8; 512];

    let s = stats();
    if s.magic != MAGIC {
        info!(
            "TEST-21: fresh start, erasing the store {:#x}..{:#x}",
            STORE.start, STORE.end
        );
        *s = Stats {
            magic: MAGIC,
            boots: 0,
            phase: Phase::Idle as u32,
            resets_in: [0; 4],
            stores_done: 0,
            inflight: NONE,
            committed: [0; KEYS],
            inflight_landed: 0,
            inflight_lost: 0,
            ecc_words: 0,
            boots_with_ecc: 0,
            max_ecc_words: 0,
            rng: 0x1234_5678,
        };
        if let Err(e) = block_on(store.erase_all()) {
            fail_with("erase_all", e);
        }
    } else {
        s.resets_in[(s.phase as usize).min(3)] += 1;
    }
    s.phase = Phase::Idle as u32;
    s.boots += 1;

    // load and check every key
    for key in 0..KEYS {
        let fetched = block_on(store.fetch_item::<&[u8]>(&mut buf, &(key as u8)));
        let committed = s.committed[key];
        let inflight = s.inflight == key as u32;
        let seq = match fetched {
            Err(e) => fail_with("fetch_item", e),
            Ok(None) => 0,
            Ok(Some(bytes)) => {
                let mut expected = [0u8; 512];
                if bytes.len() != VALUE_LEN[key] {
                    error!(
                        "key {}: {} bytes, expected {}",
                        key,
                        bytes.len(),
                        VALUE_LEN[key]
                    );
                    fail("wrong value length");
                }
                let seq = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                if bytes != value(key, seq, &mut expected) {
                    error!("key {} seq {}: {=[u8]:x}", key, seq, bytes);
                    fail("value corrupted");
                }
                seq
            }
        };
        if seq == committed {
            if inflight {
                s.inflight_lost += 1;
            }
        } else if inflight && seq == committed + 1 {
            s.committed[key] = seq;
            s.inflight_landed += 1;
        } else {
            error!(
                "key {}: loaded seq {}, committed {}, in flight: {}",
                key, seq, committed, inflight
            );
            fail("lost or stale value");
        }
    }
    s.inflight = NONE;
    let ecc = ww_ecc_flash::ecc_errors();
    if ecc > 0 {
        s.ecc_words += ecc;
        s.boots_with_ecc += 1;
        s.max_ecc_words = s.max_ecc_words.max(ecc);
        warn!(
            "boot {}: {} words with ECC errors read as zeros",
            s.boots, ecc
        );
    }
    if s.boots.is_multiple_of(100) {
        summary();
    }
    if s.boots >= BOOTS {
        info!("TEST-21 PASSED");
        summary();
        s.magic = 0; // next run starts fresh
        cortex_m::asm::delay(16_000_000); // ~1 s for the host to read RTT
        loop {
            cortex_m::asm::bkpt();
        }
    }

    // reset after a random delay, while storing
    let delay = xorshift(&mut s.rng) % MAX_DELAY_CYCLES + 1;
    cp.SYST.set_clock_source(SystClkSource::Core);
    cp.SYST.set_reload(delay);
    cp.SYST.clear_current();
    cp.SYST.enable_interrupt();
    cp.SYST.enable_counter();
    loop {
        let key = xorshift(&mut s.rng) as usize % KEYS;
        let seq = s.committed[key] + 1;
        let mut val = [0u8; 512];
        let val = value(key, seq, &mut val);
        s.inflight = key as u32;
        if let Err(e) = block_on(store.store_item(&mut buf, &(key as u8), &val)) {
            fail_with("store_item", e);
        }
        s.committed[key] = seq;
        s.inflight = NONE;
        s.stores_done += 1;
    }
}

fn fail_with<E: core::fmt::Debug>(what: &str, e: E) -> ! {
    error!("{}: {}", what, Debug2Format(&e));
    fail(what)
}

#[exception]
fn SysTick() {
    SCB::sys_reset();
}
