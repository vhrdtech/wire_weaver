## Unreleased

### 🚀 Features

- `BoardInfo::REWORKS` (STD-24, STD-98), `#[optional]` const item `MaxLen<RefVec<Rework>, MAX_REWORKS>` (16), with
  `Rework { id: ReworkId, title: Option<MaxLen<&str, 48>>, applied: bool }`: which reworks of a bring-up board unit
  are applied. `ReworkId(u16)` is the rework's number in the board revision's list; `title: None` marks a rework the
  unit's record has applied and the firmware doesn't know. Appended, so existing devices keep working (they leave it
  out with `omit = ["board.REWORKS"]`) and existing hosts never ask for it. Version 0.3.0, snapshot
  `ww_board_info_0_3_0.ron`.
- `BoardInfo::set_reworks(changes, program) -> Result<ReworkChange, ReworkError>` (STD-98), `#[optional]`: records
  reworks done on the bench, all changes of a call or none, `program = false` a dry run. `ReworkState { id, applied }`,
  `ReworkChange` (`Unchanged`, `WouldWrite`, `Written`: in effect from the next boot), `ReworkError` (`Common`,
  `UnknownRework { id }`: never stored, `Conflict { id, with }`, `ProductionRecord`). Devices that implement
  `BoardInfo` without it add `"board.set_reworks"` (their path) to `omit`.
- Module `table` (STD-98, `no_std`): `ReworkTable` / `ReworkDef` (the firmware's list and conflicting pairs),
  `ReworkSet` (a bit per number, what a record stores), `apply` (checks a request), `list` (the `REWORKS` entries,
  unknown applied numbers last), `unknown`, `known_ids` / `IdRanges` (`1-5, 7, 8`).
- `RevisionSource::Provisional` (STD-24), appended after `Assumed`: a provisional record a bench or developer wrote
  in flash, standing in for the production record. Wire-compatible: older readers get the `future_version` variant.

### ⚠️ Breaking

- Reworked (STD-24, never published): the six string properties are replaced by the const item
  `BOARD: BoardSummary`, which adds `revision_source: RevisionSource` (`Record`, `Strap { value }`, `Assumed`).
  Strings are bounded with `MaxLen`.

### 🚀 Features

- Page `docs/std_library/ww_board_info.md`, test pair `tests/board_info`.

