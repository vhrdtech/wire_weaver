# Changelog

## Unreleased

### 🚀 Features

- Feature `defmt`: `Error` implements `defmt::Format`, so a store's errors (`ww_settings`) can be logged with defmt.
- First version (SRV-19): `EccSafeFlash<F>` wraps any `NorFlash` (embassy-stm32's `Flash`) for stores: a flash word
  with an uncorrectable ECC error (power cut while programming or erasing) reads as zeros instead of raising an NMI
  (STM32G0, feature `stm32g0`, `on_nmi()` called from the NMI handler, or feature `nmi-handler` defining it) or a
  bus error (STM32H7, feature `stm32h7`, ignored under FAULTMASK + `CCR.BFHFNMIGN`). Blocking and async
  (`embedded-storage-async`) `NorFlash`, never `MultiwriteNorFlash`. `ecc_errors()` counts the words read as zeros.
  Tested on a STM32G0B1 with a sequential-storage map through 3000 resets during its stores (TEST-21); the zeros
  path itself waits for a real power cut test, H7 is not run on hardware yet.
