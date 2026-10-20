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
| `id` | Stable short id, `rework-5`, as in the board's documentation (16 bytes) |
| `title` | What it is, `cut trace to U3 pin 4` (48 bytes) |
| `applied` | Whether this unit has it |

The id is text, not an `Interned` (STD-48) index: an index changes with every firmware build and needs the ELF
to read, while a rework is a fact about the hardware that any support case must read from any unit. A device that
keeps no list leaves it out (`omit = ["board.REWORKS"]` in `ww_codegen!`, see [optional items](../api/optional_items.md)):
the client answers `NotImplementedByDevice` without asking. `ww info` and `ww report` print the list when it is served.
Added in 0.3.0 as an appended item, so older devices and hosts are unaffected.

Device-specific capability flags (a load switch present, a rail powered) stay in the device's own API: they mean
nothing on other boards.

`BoardSummary` has no `defmt::Format` (its `DateTime` has none).
