# ww_picoboot changelog

## Unreleased

### 🚀 Features

- First version (CLT-21): PICOBOOT host for the RP2350 / RP2040 boot ROM in BOOTSEL mode on nusb. `Picoboot` with
  `exclusive_access`, `exit_xip`, `flash_erase`, `write`, `read`, `reboot2`, `get_sys_info` / `chip_id`, and
  `flash()` (erase the touched sectors, write, verify, with `Progress`). `Image` from UF2 files (family filter,
  RP2350-E10 dummy block skipped) or raw binaries. `usb::list` / `usb::open` find a BOOTSEL device by its serial
  number (the chip id). `sim::SimRp2350` (feature `sim`): a simulated RP2350 boot ROM at the transfer level for tests.
