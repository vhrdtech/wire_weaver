---
level: advanced
---

# Following a reboot: RP2350 BOOTSEL

A device that updates in place can leave the job to the chip's boot ROM: the app reboots into the ROM bootloader,
the host writes the new image, the ROM boots it. On the RP2350 (and RP2040) that is BOOTSEL mode, where the ROM
offers a UF2 drive and **PICOBOOT**, the USB interface `picotool` speaks. Nothing of WireWeaver runs there, so the host
has to find the same unit again after each reboot, talk PICOBOOT to it, and find it once more when the new firmware
is up. This is CLT-21 in the [features](../features.md), and the "update stage 1" of the rePort.

```text
app (WireWeaver)        boot ROM (BOOTSEL)               app (WireWeaver), new image
 serial = chip id  ──►   serial = chip id   ──►            serial = chip id
 BootControl::reboot     PICOBOOT: erase, write,
   (Updater)               verify, REBOOT2
```

## What the firmware does

- **Report the chip id as the USB serial number**, in the boot ROM's format: the 64-bit id (`GET_INFO` SYS chip
  info, or OTP), 16 hex digits, high word first. `wire_weaver_client::follow::rp2350_serial(chip_id)` makes the
  same string on the host. Matching ignores case, as all device filters do.
- **Implement `ww_dfu::UpdateNode` and `BootControl`** (the lean app of the [ww_dfu](../std_library/ww_dfu.md)
  scenarios), with `Role::App`, `Profile::InPlace`, `Offers { boot_control: true, .. }`. `reboot(Updater)` replies
  first, then calls the ROM's `reboot` into BOOTSEL (`rom_reboot(REBOOT2_FLAG_REBOOT_TYPE_BOOTSEL, ..)`) a few tens of
  milliseconds later, so the reply leaves.
- Keep **PICOBOOT enabled** in BOOTSEL (don't pass `BOOTSEL_FLAG_DISABLE_PICOBOOT_INTERFACE`, don't disable it in OTP).

## What the host does

`follow::rp2_update()` and the `wire_weaver_client::ww_picoboot` re-export need the `picoboot` Cargo feature of
`wire_weaver_client` (off by default, pulls in `ww_picoboot` and its `nusb`-backed PICOBOOT transport): most host apps
that only talk WireWeaver over USB never flash an RP2040 / RP2350 boot ROM, so they shouldn't pay for it.
`follow::rp2_update_with()` and `follow::Follow`/`Bus`/`Found` stay available without it — that is what
`tests/follow_reboot` runs against a simulated device.

```rust
use wire_weaver_client::follow;
use wire_weaver_client::ww_picoboot::{Image, image::family};

let serial = device_info.serials[0].clone(); // as listed, before the reboot
app.boot().reboot(BootTarget::Updater).call().await?;
drop(app);

let image = Image::from_uf2(&std::fs::read("app.uf2")?, &[family::RP2350_ARM_S])?;
let back = follow::rp2_update(&serial, &image, &mut ()).await?; // DeviceInfo of the app that came back
let app = App::config(|c| c.usb().serial_eq(serial)).connect().await?;
```

`rp2_update` waits for the BOOTSEL device with the serial number (10 s), opens its PICOBOOT interface, writes and
verifies the image, sends `REBOOT2` as `picotool load -x` does (`FLASH_UPDATE` at the image's base, 500 ms delay) and
waits for a WireWeaver device with the same serial number (10 s). A timeout says what was there instead, e.g. the app
still running because it ignored the reboot request.

The pieces work on their own too:

- `follow::Follow` waits on any `follow::Bus` (`UsbBus` lists real USB devices): `gone()` until no device has the
  serial number (needed when the device reboots into the same kind of firmware; it has to stay off the bus longer than
  `Follow::poll`, 50 ms, which USB re-enumeration always does), `back()` with a predicate on `follow::Found`
  (`WireWeaver`, `RpBootsel`, `Other`), `back_in_bootsel()`, `back_in_wire_weaver()`.
- `follow::rp2_update_with()` takes the bus and the PICOBOOT opener as arguments: that is how the tests run it on a
  simulated device.
- `ww_picoboot` is the PICOBOOT host on its own: `usb::open(Some(serial))` gives a `Picoboot` with `flash()` (erase
  the touched 4 KiB sectors, write 256-byte pages, read back, with a progress callback), `read`, `write`,
  `flash_erase`, `chip_id`, `reboot2`. `Image` comes from a UF2 file (blocks of the given families; picotool's
  RP2350-E10 dummy block is skipped) or a raw binary at an address. No libusb: nusb, pure Rust. It lives in its own
  repo, [picoboot_host](https://github.com/vhrdtech/picoboot_host) (crate name `picoboot_host`, re-exported here as
  `ww_picoboot`), with no WireWeaver dependency of its own.

## From the command line

```sh
ww bootsel list                                   # boot ROMs in BOOTSEL mode with their serial numbers
ww bootsel info -s E661                           # chip id over PICOBOOT
ww bootsel flash app.uf2 -s E661                  # erase, write, verify, reboot into the new image
ww bootsel flash app.uf2 -s E661640843522C2F --wait 10 --follow
                                                  # after the app was asked to reboot(Updater): wait for the
                                                  # boot ROM, flash, wait for the app to come back
ww bootsel reboot -s E661 [--bootsel]
```

`-s` / `--serial` selects the device as for every other command (flag, `WW_SERIAL`, `ww.toml`), a part of the serial
number is enough when only one device matches; `--wait` and `--follow` need the full one. A raw binary (any extension
other than `.uf2`) is written at `--addr`, `0x10000000` by default. On Linux the boot ROM (`2e8a:000f`) needs a udev
rule for access without root, as picotool does.

The app is asked to reboot into BOOTSEL with [`ww dfu reboot`](../cli_reference.md#ww-dfu), which calls
`ww_dfu::BootControl` on any device that implements it, whatever its API and version (a global trait path, API-20 and
CLT-10):

```sh
ww dfu reboot -s E661640843522C2F                 # app -> BOOTSEL
ww bootsel flash app.uf2 -s E661640843522C2F --wait 10 --follow
```

## Tests without hardware

`ww_picoboot::sim::SimRp2350` (feature `sim`) is a boot ROM at the level of PICOBOOT's USB transfers: command
packets, data phases, zero-length status packets, stalls on refused commands, `IF_RESET` / `IF_CMD_STATUS`, flash
that only clears bits when programmed. `tests/follow_reboot` puts it under a simulated RP2350 device whose app (the
`dfu_lean_app_api` server on the in-process simulated USB) boots from that flash and reports the image version, and
runs the whole sequence: v1 → `reboot(Updater)` → BOOTSEL under the same serial → v2 written over PICOBOOT → v2 runs.
What only hardware shows: real enumeration times, the ROM's exact serial number string, `REBOOT2` behaviour with
partition tables, Windows and macOS drivers.
