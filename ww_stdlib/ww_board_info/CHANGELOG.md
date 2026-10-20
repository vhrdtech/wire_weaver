## Unreleased

### 🚀 Features

- `RevisionSource::Provisional` (STD-24), appended after `Assumed`: a provisional record a bench or developer wrote
  in flash, standing in for the production record. Wire-compatible: older readers get the `future_version` variant.

### ⚠️ Breaking

- Reworked (STD-24, never published): the six string properties are replaced by the const item
  `BOARD: BoardSummary`, which adds `revision_source: RevisionSource` (`Record`, `Strap { value }`, `Assumed`).
  Strings are bounded with `MaxLen`.

### 🚀 Features

- Page `docs/std_library/ww_board_info.md`, test pair `tests/board_info`.

