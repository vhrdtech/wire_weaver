# ecc_flash_g0b1

Hardware test TEST-21 for `mcu/ww_ecc_flash` on any STM32G0B1 board (no pins used; tested on the Donguru B153A):

- `cargo run --release`: a sequential-storage map on `EccSafeFlash` in bank 2 (`0x0807_0000..0x0807_2000`), reset
  by SysTick at a random time while it stores, checked against what earlier boots committed. After 3000 boots it
  prints the summary and `TEST-21 PASSED`, or stops at the first lost or corrupt value.
- `cargo run --release --bin reset_cut`: cuts a page program or erase (`0x0807_4000`) with a reset and prints what
  is left. On G0 a reset waits for the flash operation, so neither binary produces ECC errors: that needs a real
  power cut.

Pick the probe with `PROBE_RS_PROBE=VID:PID[:SERIAL]` when more than one is connected. Both replace the board's
firmware. See `docs/api/ecc_flash.md`.
