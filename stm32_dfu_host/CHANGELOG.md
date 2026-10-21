## Unreleased

### 🚀 Features

- First version (CLT-34): DfuSe host for the STM32 system bootloader on nusb. `usb::list`/`find`/`open`, `Dfuse` (`flash` with erase of the touched sectors, flash-word padding and read-back verify, `erase_sector`, `read`, `write`, `leave`), `MemoryLayout::parse` of the alt setting string, `Image` from an ELF (`PT_LOAD` by physical address) or a binary, `rom_serial(uid)`, and `sim::SimStm32Dfu` for tests (feature `sim`).
