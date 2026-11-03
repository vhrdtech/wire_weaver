---
level: basic
---

# ww_board_info

`ww_board_info` describes the PCB and its assembly in one value, so `ww list` and a support case need a single read.
It is `no_std` by default and uses [`ww_date_time`](ww_date_time.md) for the build date.

```rust
#[ww_trait]
pub trait BoardInfo {
    const BOARD: BoardSummary<'i>;
}
```

`BOARD` is a [`const` item](../api/properties.md#const-items), served without a handler and read by a host once per
connection. A board read from OTP or straps at boot provides it as a `fn` (`const_model = "BOARD=fn"`,
`fn board_board(&self) -> BoardSummary<'_>` for `board: BoardInfo`), a firmware for one fixed board as a Rust const.
It never fails; what a device does not know is an empty string or `None`.

| Field | Meaning |
|---|---|
| `name` | Board series, `B200` (32 bytes) |
| `revision` | Revision as printed on the board, `B` (8 bytes) |
| `revision_source` | `Record`, `Strap { value }`, `Assumed` or `Provisional` |
| `variant`, `bom_variant`, `serial` | Assembly variant, BOM variant, serial number (32 bytes each) |
| `build_date` | `Option<DateTime>` |

## Where the revision comes from

`RevisionSource` tells how the firmware knows the revision, which matters for support:

* `Record` - read from a record written in production (OTP, EEPROM, flash).
* `Strap { value }` - read from resistor straps or a revision pin at boot; `value` is the raw reading, so a host can
  show what was measured.
* `Assumed` - neither exists: the firmware assumes the revision it was built for. A unit on an assumed revision is a
  different case from one that read its record.
* `Provisional` - a provisional record a bench or developer wrote in editable storage (flash), standing in for the
  production record until it is written; not a production record. Added later as an appended variant: older hosts
  read it as their `future_version` variant.

## Reworks

Bring-up boards of one revision differ: some have a cut trace, a jumper or a swapped part that others lack. A device
that keeps the list serves it as an optional const item:

```rust
#[optional]
const REWORKS: MaxLen<RefVec<'i, Rework<'i>>, MAX_REWORKS>; // 16 entries
```

| `Rework` field | Meaning |
|---|---|
| `id` | `ReworkId(u16)`: its number in the board revision's rework list, `7` for "rework 7" |
| `title` | What it is, `cut trace to U3 pin 4` (48 bytes), from the firmware's table; `None` for a rework the unit's record has applied and this firmware doesn't know |
| `applied` | Whether this unit has it |

The list has every rework the firmware knows, in the order of the board's rework list, then the applied ones it
doesn't know. Numbers are per board revision. The id is a number, not text or an `Interned` (STD-48) index: the
unit's record keeps a bit per number, so any firmware and any support case reads the same fact from any unit, and
the titles stay in the firmware, out of the record.

A device that keeps no list leaves it out (`omit = ["board.REWORKS"]` in `ww_codegen!`, see [optional
items](../api/optional_items.md)): the client answers `NotImplementedByDevice` without asking. `ww info` and
`ww report` print the list when it is served:

```text
Reworks    rework 1: analog buffer supply (applied)
           rework 3: 1.8 V LDO (TLV70218) (not applied)
           rework 9 (unknown to this firmware): applied, allowed for this image
```

A running firmware lists a rework it doesn't know only when an allowance for its image covers it: without one it
would have refused to boot ([below](#the-rework-guard)).

### Recording a rework

After a rework on the bench, the unit's record is changed through one optional method:

```rust
#[optional]
fn set_reworks(
    changes: MaxLen<RefVec<'i, ReworkState>, MAX_REWORKS>, // { id, applied }
    program: bool,
) -> Result<ReworkChange, ReworkError<'i>>;
```

* All changes of a call or none: a swap (undo 5, apply 8) is one call, and a call with one bad number stores
  nothing.
* `program` false is a dry run: everything is checked, nothing written (`ReworkChange::WouldWrite`).
* `Unchanged` when the record already says this, `Written` otherwise: the board runs on it from its next boot.
  `REWORKS`, read once per connection, shows the board as it booted.
* A device that serves `set_reworks` serves `REWORKS` too, so a host can name what the firmware knows.

| `ReworkError` | When |
|---|---|
| `UnknownRework { id }` | The firmware doesn't know the number: it is refused, never stored. Update the firmware first. Undoing an unknown number is refused too: the firmware can't judge it |
| `Conflict { id, with }` | The result would have two reworks that can't both be applied (two ways of rewiring the same lines) |
| `ProductionRecord` | A production record fixes this unit's reworks; a bench record would never be used |
| `Common(..)` | A storage fault (`Fault`), a stored record from a newer firmware (`WrongState`), ... |

The rule that matters is the first: a firmware never stores a number it can't name. A host shows the refusal with
the numbers the firmware knows, from `REWORKS`: `unknown rework 7: this firmware knows 1-5, 8; update firmware
first`.

## The rework guard

A record written by a newer firmware can hold a rework number the running one doesn't know (flashed back to an older
build, or a rework recorded by a newer `ww board rework`). The firmware can't know what that rework changed: it can
remove, reroute or short a path, so even the plain init sequence (the default pin setup, a peripheral clock, a chain's
reset state) can damage the board. So the rule for every board is: **a firmware never runs a board with a rework it
doesn't know** (STD-101). It refuses to boot:

1. First thing in `main`, before any board init (no HAL, no pin, no peripheral clock), it reads the record and the
   allowance straight from flash.
2. If a rework applied is unknown and no allowance for this very image covers it, it blinks the shared [blink
   code](https://github.com/vhrdtech/embedded_bedrock) 1 on the status LED for about 10 s (`bedrock_led::code`: one
   long flash, one short, a pause).
3. Then it reboots into the chip's ROM bootloader (RP2350 BOOTSEL, STM32 system DFU). A reset repeats it.

There the flasher can act: flash a firmware that knows the rework, or store an **allowance**, the user's agreement that
one image may run the board with named reworks it doesn't know. The allowance is written only through the ROM
bootloader, by `ww`; the firmware only reads it.

The allowance is built so it can't get stuck:

* It is bound to one image by its `ImageId`, the first 8 bytes of the GNU build id. Any other image ignores it,
  whichever tool flashed that image (probe-rs, picotool, a UF2 copy).
* It names the numbers it covers. A further unknown rework recorded later isn't covered.
* `ww` clears it with the next flashing that needs none.

One gap is left, on purpose: the very same image flashed again by another tool finds its allowance still valid. That
is the image the user agreed to, and the next `ww` flashing clears it.

The allowance is not a security boundary. Whoever can store it can also change the record.

A board that runs with an allowance lists the unknown numbers in `REWORKS` with `title: None`; nothing else of the API
changes. The boot refusal is the safety; the flasher's check below is the comfort: a board flashed by any tool still
never runs an unknown rework.

### Where the record is: the image table

A board that refuses to boot sits in its ROM bootloader, where no firmware answers. So every guarding image carries
a small table in flash (`ww_board_info::image`) that says, without a round trip to any firmware:

* where the board's bench record and the allowance are: a `Locator` naming the `ww_settings` store's flash range,
  word and page size, the record's key and the slots of its revision and reworks, and the allowance's key;
* per board and revision, the reworks the image knows.

It is built from the same consts the firmware uses:

```rust
const RECORD: Locator = Locator {
    flash_base: 0x1000_0000,                            // flash offset 0 in memory (RP2350 XIP)
    store_start: 0x7c000, store_len: 0x4000,            // the settings store, as the flash driver counts
    page_size: 4096, word_size: 1,
    record_key: 1, revision_slot: 0, reworks_slot: 1, reworks_width: 4, // the board's own `board` group
    allowance_key: 2,                                   // guard::AllowanceGroup
};
ww_board_info::image_reworks!(pub static IMAGE_REWORKS = RECORD, [
    ("B200", "A", REWORKS_B200A),
    ("B200", "B", NO_REWORKS),
]);
```

The firmware's boot check reads its own table through the same bytes (`IMAGE_REWORKS.table()`), so the record a
flasher reads and the one the firmware decides on are the same, and the linker can't drop it. The layout (magic
`WWREWORK`, CRC-32, a 48-byte header with the locator, 48-byte entries) is in the `image` module's docs.

### Flashing: checked in the ROM bootloader

A flasher compares before it writes, with the chip in its ROM bootloader (a running board is rebooted there first):

1. It reads the reworks the new image knows, its locator and its build id from the image file.
2. It finds the table of the image in flash (read through the ROM from the start of flash until the table or erased
   flash): the board's name when that image names one board.
3. It reads the store's range through the ROM (PICOBOOT read on the RP2350, DfuSe upload on an STM32) and decodes the
   bench record and the allowance with the real `ww_settings` store (`ww_board_info::record`, feature `host`).
4. If the image lacks an applied rework, the user sees the list and must agree. Only then is the allowance stored for
   the new image's `ImageId`, else a stored one is cleared; then the image is written.

Writing the allowance runs the real store over the bytes read, and replays its own flash operations (page erase, word
program) through the ROM in the same order: a cut USB cable leaves what a power cut on the device would.

`ww dfu rp flash` and `ww dfu stm32 flash` do this ([checking the board's
reworks](../cli_reference.md#checking-the-boards-reworks), TOOL-29). `ww_report::rework_check` is the same check as a
library for other flashers: they give it a `RomFlash` (read, and replay erases and writes).

`image::check(table, name, revision, applied)` is the one comparison every flasher makes:

| `Check` | Meaning |
|---|---|
| `Ok` | The image knows every applied rework: flash, and clear a stored allowance |
| `Lacking(set)` | The image doesn't know these: ask the user, store the allowance for it, then flash |
| `OtherBoard` | The image lists other boards only: probably the wrong image |
| `NoRecord` | The image predates the guard: it doesn't check and would run the board as built, ignoring the applied reworks |

`ww_firmware_info::find_build_id` finds the image's build id in the same bytes.

### Firmware from before the guard

Deployed firmware can't learn this. The rule holds from the first firmware of a board that has the guard. Firmware
with STD-98 but without the guard runs on whatever it does with an unknown rework; firmware from before STD-98 can't
even report its reworks. The flasher's check doesn't need them: it reads the record through the ROM.

### In firmware: `ww_board_info::table`

`table` answers both items from the firmware's own table, so every board answers the same way (`no_std`):

```rust
use ww_board_info::{ReworkId, table::{ReworkDef, ReworkSet, ReworkTable}};

const REWORKS: ReworkTable = ReworkTable::new(
    &[ReworkDef::new(1, "analog buffer supply"), ReworkDef::new(5, "SD full swap"), ReworkDef::new(8, "SD short swap")],
    &[(ReworkId(5), ReworkId(8))], // can't both be applied
);
```

* `ReworkSet` is what the record stores, a bit per number (0-63).
* `REWORKS.apply(stored, changes)` checks a request and returns the new set or the `ReworkError`.
* `REWORKS.list(stored)` gives the `REWORKS` entries, unknown numbers last.
* `REWORKS.unknown(stored)` is non-empty when the board must refuse to boot (without an allowance).
* `REWORKS.known_ids()` prints `1, 5, 8` for messages.

At boot, the guard decides once (`ww_board_info::guard`), first thing in `main`:

```rust
let flash = unsafe { MappedFlash::<1, 4096>::new(0x1000_0000, FLASH_SIZE) }; // ww_settings::mapped: no driver yet
let own = ImageId::from_build_id(ww_firmware_info::build_id());               // None without the build-id note
let table = IMAGE_REWORKS.table().unwrap();
match embassy_futures::block_on(guard::boot_check(&table, flash, "B200", own)) {
    Ok((Guard::Run | Guard::RunAllowed { .. }, _)) => {}                      // init the board, run
    Ok((Guard::Refuse { .. }, _)) => refuse(Code::UnknownRework),             // blink, ROM bootloader
    Err(_) => refuse(Code::RecordUnreadable),
}
```

`boot_check` reads the record where the table's locator says (`guard::read_bench`), with the board's revision picks
the reworks the image knows, and calls `guard::decide(known, applied, allowance, own)`. The allowance is
`guard::AllowanceGroup`, two `u64` in the board's settings store: `ImageId` as a little-endian `u64` (0: none) and a
`ReworkSet`. A board that guards links the GNU build-id note (`ww_firmware_info` docs); without it no allowance can be
its own, so the board refuses until a firmware that knows the rework is flashed.

`tests/board_info` has a complete server; `ww_board_info`'s `record` tests run the boot check on a store as a device
left it.

Device-specific capability flags (a load switch present, a rail powered) stay in the device's own API: they mean
nothing on other boards.

### `ww info` and `ww board rework allow|disallow` in the boot ROM

A board that refuses to boot has no running firmware to ask, so `ww info` and `ww board rework allow`/`disallow`
read and write the bench record and the allowance the same way the flashers do: through the ROM bootloader
(PICOBOOT read on the RP2350, DfuSe upload on an STM32), not through `BoardInfo` (TOOL-30).

* `ww info` on a board in BOOTSEL / the STM32 bootloader finds the installed image's own table (build id, the
  boards it lists), reads the bench record through the same `Locator`, and prints the verdict a reboot would reach
  right now: `guard::decide` run on what it read (`Guard::Run`/`RunAllowed`/`Refuse`, the same decision `boot_check`
  makes). On a running board nothing changes: `REWORKS` is as before, an entry with `title: None` meaning the
  allowance already covers it.
* `ww board rework allow N...` stores the allowance for the installed image's `ImageId` (refused without a build-id
  note); the numbers must cover every rework the verdict calls unknown (covering more is fine, the allowance's own
  rule). `disallow` clears it. Both refuse when the installed image names no bench record: nothing to check.
* The guard runs once, at boot: `ww info` only shows the new verdict after the board reboots into the image.

`ww_report::rework_check` has this as a library (`scan_installed_image`, `read_board`, `board_guard`,
`rom_info_text`, `set_allowance`), the same one [TOOL-29's flasher
check](../cli_reference.md#checking-the-boards-reworks) uses; `wire_weaver_cli::cmd::rom` holds the `RomFlash`
adapters (`PicobootRom`, `DfuseRom`) every ROM command shares. `ww dfu rp flash --follow` / `ww dfu stm32 flash
--follow` tell an image that refused to boot (back in the ROM within about 12 s instead of as WireWeaver) from one
that runs, and point at `ww info`.

`BoardSummary` has no `defmt::Format` (its `DateTime` has none).
