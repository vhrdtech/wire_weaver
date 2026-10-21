# ww_ecc_flash

STM32 `NorFlash` wrapper for stores (sequential-storage maps, settings, logs) in the MCU's own flash: a flash word
with an uncorrectable ECC error, left by a power cut while it was programmed or its page erased, reads as zeros
instead of an NMI (STM32G0) or a bus error (STM32H7). Stores skip a word of zeros like any other cut write, so the
device still boots. In strict mode (`EccSafeFlash::new_strict`, for bootloaders and image checks) the read fails
with `Error::Ecc` instead.

Docs: [ECC-safe flash for stores](../../docs/api/ecc_flash.md). Hardware tests: `examples_mcu/ecc_flash_g0b1`,
`examples_mcu/ecc_flash_h7`.
