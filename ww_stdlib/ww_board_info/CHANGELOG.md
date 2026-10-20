## Unreleased

### 🚀 Features

- `BoardInfo::REWORKS` (STD-24), `#[optional]` const item `MaxLen<RefVec<Rework>, MAX_REWORKS>` (16), with
  `Rework { id: MaxLen<&str, 16>, title: MaxLen<&str, 48>, applied: bool }`: which reworks of a bring-up board unit are
  applied. Appended, so existing devices keep working (they leave it out with `omit = ["board.REWORKS"]`) and
  existing hosts never ask for it. Version 0.3.0, snapshot `ww_board_info_0_3_0.ron`.
- `RevisionSource::Provisional` (STD-24), appended after `Assumed`: a provisional record a bench or developer wrote
  in flash, standing in for the production record. Wire-compatible: older readers get the `future_version` variant.

### ⚠️ Breaking

- Reworked (STD-24, never published): the six string properties are replaced by the const item
  `BOARD: BoardSummary`, which adds `revision_source: RevisionSource` (`Record`, `Strap { value }`, `Assumed`).
  Strings are bounded with `MaxLen`.

### 🚀 Features

- Page `docs/std_library/ww_board_info.md`, test pair `tests/board_info`.

