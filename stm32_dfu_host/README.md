# stm32_dfu_host

> Host for the STM32 system bootloader's USB DFU (DfuSe) in Rust on nusb: list, read the memory layout, erase,
> write with read-back verify, jump to flash

Implements the STM32 system bootloader's USB DFU protocol (DfuSe, AN3156) for devices enumerating as
`0483:DF11`. `Dfuse` is the protocol on top of the sans-IO `DfuIo` trait, implemented for real devices on `nusb`
(feature `usb`) and for tests by a simulated device (feature `sim`), so erase/write/verify logic is exercised
without hardware. Chosen over the existing `dfu-core` crate, which owns the download loop and the control over
erase timeouts (seconds per 128 KB sector on an H7) and has no upload path, so no read-back verify and no sans-IO
boundary to put a simulated device behind.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture.
