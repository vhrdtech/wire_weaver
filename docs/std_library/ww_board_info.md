# ww_board_info

`ww_board_info` describes the PCB and its assembly in one value, so `ww list` and a support case need a single read.
It is `no_std` by default and uses [`ww_date_time`](ww_date_time.md) for the build date.

```rust
#[ww_trait]
pub trait BoardInfo {
    const BOARD: BoardSummary<'i>;
}
```

`BOARD` is a `const` item: until constant items are served from the introspection data (API-36) a server implements
it as `get_<path>_BOARD`. It never fails; what a device does not know is an empty string or `None`.

| Field | Meaning |
|---|---|
| `name` | Board series, `B200` (32 bytes) |
| `revision` | Revision as printed on the board, `B` (8 bytes) |
| `revision_source` | `Record`, `Strap { value }` or `Assumed` |
| `variant`, `bom_variant`, `serial` | Assembly variant, BOM variant, serial number (32 bytes each) |
| `build_date` | `Option<DateTime>` |

## Where the revision comes from

`RevisionSource` tells how the firmware knows the revision, which matters for support:

* `Record` - read from a record written in production (OTP, EEPROM, flash).
* `Strap { value }` - read from resistor straps or a revision pin at boot; `value` is the raw reading, so a host can
  show what was measured.
* `Assumed` - neither exists: the firmware assumes the revision it was built for. A unit on an assumed revision is a
  different case from one that read its record.

Device-specific capability flags (a load switch present, a rail powered) stay in the device's own API: they mean
nothing on other boards.

`BoardSummary` has no `defmt::Format` (its `DateTime` has none).
