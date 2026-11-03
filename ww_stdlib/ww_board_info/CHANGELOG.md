## Unreleased

### ⚠️ Breaking

- The rework guard (STD-101) replaces "runs on its safe features": a firmware with an applied rework it doesn't know
  boots guarded. Devices implementing `BoardInfo` without the guard add `"board.GUARD"`,
  `"board.allow_unknown_reworks"` and `"board.clear_unknown_reworks_allowance"` (their paths) to `omit`.
  `ReworkError::Guarded { id }` appended. Still 0.3.0 (never published).

### 🚀 Features

- `image::ImageTable::as_bytes`: the record's bytes, to keep a found record beyond the image buffer (TOOL-29).
- `BoardInfo::GUARD` (STD-101), `#[optional]` const item `GuardSummary { state: GuardState, unknown, allowance:
  Option<Allowance> }`: `GuardState` `Normal`, `Guarded` (identity, info, updates and reading the record only, outputs
  safe), `Allowed` (unknown reworks covered by an allowance for this image).
- `BoardInfo::allow_unknown_reworks(image: ImageId, reworks, program)` and `clear_unknown_reworks_allowance(program)`
  (STD-101), `#[optional]`: store or erase the allowance that lets one image run with named unknown reworks.
  `ImageId([u8; 8])` is the first 8 bytes of the image's GNU build id (`ImageId::from_build_id`, `Display` as hex).
- Module `guard` (STD-101, `no_std`): `decide(known, applied, allowance, own) -> Decision { guard: Guard,
  erase_allowance }`, `StoredAllowance` (two `u64`, what a board stores), `allowance_from_request`, `GuardReport`
  (serves `GUARD`).
- Module `image` (STD-101): the record of the reworks an image knows, in the image (magic `WWREWORK`, CRC-32, a
  48-byte entry per board revision). `image_reworks!` builds it as a `static` from `ReworkTable` consts,
  `ImageReworks::known` reads it back in firmware, `ImageTable::find` / `parse` / `entries` / `known` read it from
  image bytes, `check` (`Check::Ok`, `Lacking`, `OtherBoard`, `NoRecord`) is the comparison every flasher makes.

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

