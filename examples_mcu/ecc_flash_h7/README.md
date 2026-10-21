# ecc_flash_h7

Hardware check of `mcu/ww_ecc_flash` on an STM32H72x/H73x (SRV-19, chunky ECC-2): makes real uncorrectable ECC
errors and reads them in both modes. Built for the STM32H725IG (B135, the omarchy-m1 board); another H7 needs its
chip feature, runner chip and the scratch sector in `src/main.rs` and `memory.x`.

A flash word is 32 bytes with its own ECC bits, and programming only clears bits: a word programmed twice holds
the AND of both data and the AND of both ECCs, which mostly don't match. The test erases the last sector
(`0x080E_0000`, 128 KiB), programs every even word once (control) and every odd word twice (two random patterns,
a few with all zeros as the second), then checks:

- through `EccSafeFlash::new` (zeros): control words read right, a word that fails ECC reads as zeros and counts in
  `ecc_errors()`, a word that passes reads as the AND of both patterns; it also reports the words the flash
  corrected (`SNECCERR`) and whether a second program of all zeros reads clean, the one second write chunky allows;
- through `EccSafeFlash::new_strict`: the same words fail with `Error::Ecc { offset }`, a read of the whole range
  stops at the first of them, control words read right;
- after an erase every word reads `0xFF` in strict mode.

It ends with `ECC-STRICT H7 PASSED` (and the core halts, probe-rs exits) or a panic naming the word that broke a
rule. It needs at least one word with an uncorrectable error: if the flash refuses the second program instead
(an error from the write), the test says so and stops.

```sh
cd examples_mcu/ecc_flash_h7
PROBE_RS_PROBE=1366:0105 cargo run --release
```

It replaces the board's firmware (and a bootloader at `0x0800_0000`) and erases `0x080E_0000..0x0810_0000`. See
`docs/api/ecc_flash.md`.
