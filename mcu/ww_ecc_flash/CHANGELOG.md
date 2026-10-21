# Changelog

## Unreleased

### 🚀 Features

- Strict mode (SRV-19) for bootloaders and image checks: `EccSafeFlash::new_strict` gives an `EccStrictFlash<F>`
  (`EccSafeFlash<F, Strict>`) whose read fails with the new `Error::Ecc { offset }` at the first flash word that
  fails its ECC check, instead of reading it as zeros; `Error::is_ecc()`. The mode is a type parameter
  (`EccSafeFlash<F, M: Mode = Zeros>`), `EccSafeFlash::new` keeps the zeros mode. `ecc_errors()` counts the
  failed words of both modes. Host tests on a simulated flash (`just test-ecc-flash`). A `match` over `Error` needs
  an arm for `Error::Ecc`.
- Feature `defmt`: `Error` implements `defmt::Format`, so a store's errors (`ww_settings`) can be logged with defmt.
- First version (SRV-19): `EccSafeFlash<F>` wraps any `NorFlash` (embassy-stm32's `Flash`) for stores: a flash word
  with an uncorrectable ECC error (power cut while programming or erasing) reads as zeros instead of raising an NMI
  (STM32G0, feature `stm32g0`, `on_nmi()` called from the NMI handler, or feature `nmi-handler` defining it) or a
  bus error (STM32H7, feature `stm32h7`, ignored under FAULTMASK + `CCR.BFHFNMIGN`). Blocking and async
  (`embedded-storage-async`) `NorFlash`, never `MultiwriteNorFlash`. `ecc_errors()` counts the words read as zeros.
  Tested on a STM32G0B1 with a sequential-storage map through 3000 resets during its stores (TEST-21); the zeros
  path itself waits for a real power cut test, H7 is not run on hardware yet.
