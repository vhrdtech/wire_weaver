## Unreleased

### ⚠️ Breaking

* `#[since]` markers are wire revisions (EVO-24): `RevisionSource::Provisional` `w1.1`, `REWORKS` and `set_reworks` `w1.2` (were `0.2.0` and `0.3.0`). The signatures of `BoardInfo` and `RevisionSource` change with the marker bytes; the crate gets its own wire version with EVO-26.
- The rework guard (STD-101) replaces "runs on its safe features": a firmware with an applied rework it doesn't know
  refuses to boot, before any init, unless an allowance for its very image covers it. No API items for it: the
  first plan's `GUARD`, `allow_unknown_reworks`, `clear_unknown_reworks_allowance`, `ReworkError::Guarded`,
  `GuardSummary`, `GuardState` and `Allowance` went before any release. Still 0.3.0 (never published).

### 🚀 Features

- Module `guard` (STD-101, `no_std`): `boot_check(table, flash, name, own) -> (Guard, BenchRecord)`, the check every
  guarding firmware runs first thing in `main` (through the image table's locator, on a
  `ww_settings::mapped::MappedFlash` before any driver); `read_bench` (`BenchRecord`: revision, reworks, allowance;
  `ReadError`); `decide(known, applied, allowance, own) -> Guard` (`Run`, `RunAllowed`, `Refuse`);
  `ImageId([u8; 8])`, the first 8 bytes of the image's GNU build id (`from_build_id`, `Display` as hex, re-exported at
  the crate root); `StoredAllowance` (two `u64`, `for_reworks`, `covers`); `AllowanceGroup`, the allowance as a
  `ww_settings` group.
- Module `image` (STD-101), format 2: the table in the image (magic `WWREWORK`, CRC-32, a 48-byte header with the
  `Locator` of the board's bench record and the allowance, a 48-byte entry per board revision with the reworks the
  image knows). `image_reworks!(static X = LOCATOR, [..])` builds it from `ReworkTable` consts, `ImageReworks::table`
  / `known` read it back in firmware, `ImageTable::find` / `parse` / `entries` / `known` / `locator` /
  `board_names` / `as_bytes` read it from image bytes, `check` (`Check::Ok`, `Lacking`, `OtherBoard`, `NoRecord`)
  is the comparison every flasher makes. Format 1 (no locator) is not read.
- Module `record` (STD-101, feature `host`): the bench record and the allowance through a boot ROM. `read(locator,
  store_bytes)`, `set_allowance(locator, store_bytes, allowance) -> Change { ops, image }`: the store's own erases and
  writes at memory addresses, to replay through the ROM (TOOL-29).

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

