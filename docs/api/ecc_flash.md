---
level: advanced
---

# ECC-safe flash for stores

Stores in the MCU's own flash (settings, logs, a sequential-storage map) are written for power cuts: a record cut
in the middle fails its CRC and is skipped, a half-erased page is erased again. On STM32 parts with flash ECC there
is one more outcome of a power cut, and it stops a device from booting unless the flash driver handles it.

## The problem

Flash with ECC stores a few check bits next to every flash word: 8 bytes on STM32G0/G4/L4, 32 bytes on
STM32H74x/H75x/H72x/H73x (16 on H7A3/B0). A power cut while a word is programmed, or while its page is erased, can
leave data and check bits that don't match: an **uncorrectable ECC error**. Reading that word doesn't return data:

| Family | What a read of a broken word does | Default outcome |
|---|---|---|
| STM32G0 | NMI with `FLASH_ECCR.ECCD` (bank 2: `FLASH_ECCR2`) set | the default NMI handler hangs, a watchdog reset reads the same word again: boot loop |
| STM32H7 | bus error, `FLASH_SR.DBECCERR` set | HardFault, same boot loop |

embassy-stm32's flash read is a plain `memcpy`, and sequential-storage passes a read error up as
`Error::Storage` without skipping or repairing anything. One cut write, and the device never gets past loading its
settings.

## `EccSafeFlash`

`ww_ecc_flash` (crate in the `mcu/` workspace) wraps any `embedded-storage` `NorFlash`, e.g. embassy-stm32's
`Flash<'_, Blocking>`, and reads the flash itself, one ECC word at a time. A word that fails its ECC check reads as
**zeros**. Stores already handle a word of zeros, because it is what they see after other cut writes:

- an item header of zeros fails its CRC and is skipped word by word;
- item data of zeros fails the item's CRC, the item is skipped;
- a page marker of zeros counts as written, a legal outcome of a cut marker write.

Writes and erases go to the wrapped flash unchanged. `EccSafeFlash` implements the blocking (`embedded-storage`) and
the async (`embedded-storage-async`) `NorFlash` traits, so it fits sequential-storage directly. It never implements
`MultiwriteNorFlash`: programming a word twice is not allowed with ECC.

```rust
use embassy_stm32::flash::{FLASH_BASE, Flash};
use ww_ecc_flash::EccSafeFlash;

let flash = Flash::new_blocking(p.FLASH);
// SAFETY: offsets of embassy's `Flash` start at FLASH_BASE
let flash = unsafe { EccSafeFlash::new(flash, FLASH_BASE as u32) };
let mut map = MapStorage::<u8, _, _>::new(flash, const { MapConfig::new(0x7_0000..0x7_2000) }, Cache::new_uncached());
```

`ww_ecc_flash::ecc_errors()` counts the words read as zeros since boot, worth reporting in device diagnostics.

### STM32G0: the NMI handler

Enable feature `stm32g0`. The NMI handler has to give `ww_ecc_flash` the first look:

```rust
#[cortex_m_rt::exception]
unsafe fn NonMaskableInt() {
    if ww_ecc_flash::on_nmi() {
        return; // a word read through EccSafeFlash failed its ECC check, it reads as zeros
    }
    // any other NMI: the application's own handling
}
```

Or enable feature `nmi-handler` (implies `stm32g0`), which defines this handler and panics on any other NMI.
`on_nmi()` only takes an ECC error at the address `EccSafeFlash` is reading at that moment (it compares the
`ADDR_ECC` field with it); an ECC error in code, constants or another driver's read is left to the application.

### STM32H7: no handler

Enable feature `stm32h7`. Each flash word is read with all exceptions masked (`FAULTMASK`) and `SCB.CCR.BFHFNMIGN`
set, so the bus error is ignored; then `DBECCERR` of the word's bank is checked and cleared. With the D-cache on, the
cache lines of the word are invalidated before the read. Keep the store region non-cacheable anyway (MPU): a bus
error on a cache linefill may be reported imprecisely, and an imprecise bus error is not ignored.

## Tested so far

- STM32G0B1 (Donguru B153A), `examples_mcu/ecc_flash_g0b1`: a sequential-storage map on `EccSafeFlash` in bank 2,
  reset by SysTick at a random time while it stores, 3000 boots: every committed value loads, nothing corrupt.
- What a reset does to a flash operation on G0 (`src/bin/reset_cut.rs` there): a system reset waits for it. Erases
  cut 1-20 ms into their ~22 ms still leave the page fully erased, programs stop between double words. So a reset
  never produces an ECC error on G0: only a real power cut does, and the zeros path is still to be run on hardware
  with one (TEST-21 in [Features](../features.md)).
- STM32H7: built, not run on hardware.

## Running the hardware test

The test runs on any STM32G0B1 board (it uses no pins). With probe-rs and the board's probe:

```sh
cd examples_mcu/ecc_flash_g0b1
PROBE_RS_PROBE=1366:0105 cargo run --release                 # the store test, ends with "TEST-21 PASSED"
PROBE_RS_PROBE=1366:0105 cargo run --release --bin reset_cut  # what a reset leaves of a program / erase
```

It erases and writes flash at `0x0807_0000..0x0807_2000` and `0x0807_4000..0x0807_4800`, and replaces the firmware.
