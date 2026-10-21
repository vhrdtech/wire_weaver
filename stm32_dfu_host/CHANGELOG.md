## Unreleased

### 🚀 Features

- First version (CLT-34): DfuSe host for the STM32 system bootloader on nusb. `usb::list`/`find`/`open`, `Dfuse` (`flash` with erase of the touched sectors, flash-word padding and read-back verify, `erase_sector`, `read`, `write`, `leave`), `MemoryLayout::parse` of the alt setting string, `Image` from an ELF (`PT_LOAD` by physical address) or a binary, `rom_serial(uid)`, and `sim::SimStm32Dfu` for tests (feature `sim`).
- Tested on a G0B1-like layout (2 KiB pages, 8-byte flash words) next to the H72x one.
- `Dfuse::bootloader_version` (get commands) and `rom_id` (byte at `proto::ROM_ID_ADDR`); a sector size without a unit letter in the layout string is bytes (`@Option Bytes /0x5200201C/01*88 e`).
