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
           rework 9 (unknown to this firmware): applied
Guard      GUARDED: unknown rework 9; flash a firmware that knows it, or ww board rework allow 9
```

The guard line comes from `GUARD` ([below](#the-rework-guard)).

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
remove or reroute a path the firmware would drive. So the rule for every board is: **a firmware never runs a board
with a rework it doesn't know** (STD-101). It boots guarded instead:

* It serves identity, `ww info`, updates and reading the record only. Everything else is refused with the reason
  (SRV-50's gate).
* Every output stays in its safe state: no slot, bus or peripheral of the board is started.
* It reports it in `GUARD`, and on a status LED where the board has one.

A guarded board is left by flashing a firmware that knows the rework, or by an **allowance**: the user agrees that one
image may run the board with named reworks it doesn't know.

```rust
#[optional]
const GUARD: GuardSummary<'i>; // state, unknown numbers, the stored allowance

#[optional]
fn allow_unknown_reworks(
    image: ImageId,                                    // first 8 bytes of the image's GNU build id
    reworks: MaxLen<RefVec<'i, ReworkId>, MAX_REWORKS>, // the unknown ones it may run with
    program: bool,
) -> Result<ReworkChange, ReworkError<'i>>;

#[optional]
fn clear_unknown_reworks_allowance(program: bool) -> Result<ReworkChange, ReworkError<'i>>;
```

| `GuardState` | When |
|---|---|
| `Normal` | The firmware knows every rework applied |
| `Guarded` | A rework applied is unknown, and no allowance for this image covers it |
| `Allowed` | Unknown reworks, but the allowance for this image covers every one: the board runs normally |

The allowance is built so it can't get stuck:

* It is bound to one image by its `ImageId`, the first 8 bytes of the GNU build id. Any other image ignores it,
  whichever tool flashed that image (probe-rs, picotool, a UF2 copy).
* It names the numbers it covers. A further unknown rework recorded later isn't covered.
* An image with the guard erases an allowance that isn't its own at boot. So it can't come back when the old image is
  flashed again later.
* `ww` flashers clear it before writing an image that needs none.

One gap is left, on purpose: the very same image flashed again by another tool, with only firmware from before the
guard in between, finds its allowance still valid. That is the image the user agreed to.

The allowance is not a security boundary. Whoever can store it can also change the record.

`set_reworks` is refused while guarded (`ReworkError::Guarded { id }`): record reading only. `allow_unknown_reworks`
and `clear_unknown_reworks_allowance` are served in every state.

### Flashing: the image knows its reworks

A flasher compares before it writes:

1. It reads the board's name, revision and applied reworks from the running device (`BOARD`, `REWORKS`, `GUARD`).
2. It reads the reworks the new image knows from the image itself: a small record every guarding image carries in
flash (`ww_board_info::image`).
3. If the image lacks an applied rework, the user sees the list and must agree. Only then is the allowance stored for
the new image's `ImageId`, and the image written.

`ww dfu rp flash`, `ww dfu stm32 flash` and `ww dfu flash` do this (TOOL-29). When the device can't be read before
writing (already in the boot ROM, or running firmware from before the guard that can't store an allowance), the new
image guards itself. `ww board rework allow N` then stores the allowance for the running image, and the board runs
normally from its next boot (TOOL-30).

The record is built from the same `ReworkTable` consts the firmware uses:

```rust
ww_board_info::image_reworks!(pub static IMAGE_REWORKS = [
    ("B200", "A", REWORKS_B200A),
    ("B200", "B", NO_REWORKS),
]);
```

`ImageTable::find(bytes)` finds it in an image's flash contents (ELF, UF2 or raw binary, once loaded).
`image::check(table, name, revision, applied)` is the one comparison every flasher makes:

| `Check` | Meaning |
|---|---|
| `Ok` | The image knows every applied rework: flash, and clear a stored allowance |
| `Lacking(set)` | The image doesn't know these: ask the user, store the allowance for it, then flash |
| `OtherBoard` | The image lists other boards only: probably the wrong image |
| `NoRecord` | The image predates the guard: it doesn't guard and would run the board as built, ignoring the applied reworks |

The record's layout (magic `WWREWORK`, CRC-32, 48-byte entries) is in the `image` module's docs.
`ww_firmware_info::find_build_id` finds the image's build id in the same bytes.

### Firmware from before the guard

Deployed firmware can't learn this. The rule holds from the first firmware of a board that has the guard. Firmware
with STD-98 but without the guard serves `REWORKS` with the unknown numbers and can't store an allowance. Firmware
from before STD-98 can't even report its reworks. A flash from either takes the post-flash path above.

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
* `REWORKS.unknown(stored)` is non-empty while the board must boot guarded.
* `REWORKS.known_ids()` prints `1, 5, 8` for messages.

At boot, the guard decides once (`ww_board_info::guard`):

```rust
let own = ImageId::from_build_id(ww_firmware_info::build_id()); // None without the build-id note
let known = IMAGE_REWORKS.known(board_name, revision);           // read through the image's own record
let decision = guard::decide(known, applied, stored_allowance, own);
if decision.erase_allowance {
    // erase the stored allowance: it is for another image
}
if decision.guard.runs_board() {
    // start the board's functions
} else {
    // guarded: USB / the link, identity, info, DFU, reading the record; outputs safe
}
let report = GuardReport::new(decision.guard, allowance_left, own); // serves GUARD: report.summary()
```

The allowance is two `u64` next to the record (`StoredAllowance`): `ImageId` as a little-endian `u64` (0: none) and a
`ReworkSet`. `guard::allowance_from_request` turns an `allow_unknown_reworks` call into one. A board that guards links
the GNU build-id note (`ww_firmware_info` docs); without it no allowance can be its own, so the board stays guarded
until a firmware that knows the rework is flashed.

`tests/board_info` has a complete server, guard included.

Device-specific capability flags (a load switch present, a rail powered) stay in the device's own API: they mean
nothing on other boards.

`BoardSummary` has no `defmt::Format` (its `DateTime` has none).
