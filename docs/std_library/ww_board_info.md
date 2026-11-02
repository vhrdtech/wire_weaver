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
           rework 9 (unknown to this firmware): applied, the board runs on its safe features until a firmware that knows it
```

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

### A record with reworks the firmware doesn't know

A record written by a newer firmware can hold a number the running one doesn't know (flashed back to an older
build). The firmware then neither ignores the record nor that number: it can't know what the rework changed (a rework
can remove a path the old firmware would drive). It serves the number with `title: None`, reports it where the device
reports its record's state, and runs the board on its safe features until a firmware that knows the rework is
flashed: what is safe on every unit of the revision, whatever its reworks.

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
* `REWORKS.unknown(stored)` is non-empty while the board must run on its safe features.
* `REWORKS.known_ids()` prints `1, 5, 8` for messages.

`tests/board_info` has a complete server.

Device-specific capability flags (a load switch present, a rail powered) stay in the device's own API: they mean
nothing on other boards.

`BoardSummary` has no `defmt::Format` (its `DateTime` has none).
