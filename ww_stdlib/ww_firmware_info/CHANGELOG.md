## Unreleased

### ⚠️ Breaking

- Reworked (STD-24, never published): `crc`, `sha256` and the `bedrock_build_info` stream are gone. `FirmwareInfo` is
  now the const items `FIRMWARE: FirmwareSummary`, `BUILD_ID` (GNU build id, the firmware's identity) and
  `#[optional] BUILD_RECORD` (the `bedrock_build_info` compact record). Servers implement `get_<path>_FIRMWARE` etc.
  (`ww api scaffold` lists them); image digests live in `ww_dfu`.

### 🚀 Features

- `FirmwareSummary` (name, `ww_version::Version`, `GitInfo` commit and dirty flag, build time, `BuildProfile`).
- `build` feature: `build::emit()` for a firmware's `build.rs` (git, version, `SOURCE_DATE_EPOCH`) and the
  `firmware_summary!()` macro that builds the summary from it. `build-id` feature: `build_id()` reads
  `.note.gnu.build-id` through `__build_id_start` / `__build_id_end`; `parse_build_id_note()` parses a note anywhere.
- New trait `LastReset`: `last_reset() -> ResetRecord` (cause, build id, uptime, `Fault`, up to 32 stack words,
  message up to 128 bytes), `boot_count()`, `clear()`. `Fault` has one variant per architecture behind the `cortex-m`
  and `riscv` features (`std` enables both), numbers fixed by position (SW-54).
- Page `docs/std_library/ww_firmware_info.md`, test pair `tests/firmware_info`.

