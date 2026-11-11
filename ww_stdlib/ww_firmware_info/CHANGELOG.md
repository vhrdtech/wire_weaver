## Unreleased

### 🚀 Features

- `find_build_id(image)` (STD-101): the GNU build id inside an image's bytes (raw binary, an ELF's or UF2's flash
  contents), found by its note header; `None` for none or two. A flasher binds a rework allowance to it.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

### ⚠️ Breaking

- Reworked (STD-24, never published): `crc`, `sha256` and the `bedrock_build_info` stream are gone. `FirmwareInfo` is
  now the const items `FIRMWARE: FirmwareSummary`, `BUILD_ID` (GNU build id, the firmware's identity) and
  `#[optional] BUILD_RECORD` (the `bedrock_build_info` compact record). Servers provide them without handlers, in
  their consts impl (API-36, `BUILD_ID` as a `fn`; `ww api scaffold` prints it); image digests live in `ww_dfu`.

- `ResetRecord::boots_since_crash` (STD-27, never published): boots since the crash data was saved, 0 = the last
  reset was the crash. Servers fill it; hosts use it to tell a fresh crash from an old one kept across resets.

- `LastReset::boot_count` and `ResetRecord::boots_since_crash` are `Count<units::Boot, u32>` instead of a bare `u32`
  (never published): a boot counter, not a plain number. Same bytes on the wire, no wire bump; read the number back
  with `.get()`.

### 🚀 Features

- `firmware_summary!()` evaluates at compile time: `const FIRMWARE_FIRMWARE: FirmwareSummary<'static> =
  firmware_summary!();` in the server's consts impl (API-36). `bounded_str` is a `const fn`.
- `FirmwareSummary` (name, `ww_version::Version`, `GitInfo` commit and dirty flag, build time, `BuildProfile`).
- `build` feature: `build::emit()` for a firmware's `build.rs` (git, version, `SOURCE_DATE_EPOCH`) and the
  `firmware_summary!()` macro that builds the summary from it. `build-id` feature: `build_id()` reads
  `.note.gnu.build-id` through `__build_id_start` / `__build_id_end`; `parse_build_id_note()` parses a note anywhere.
- New trait `LastReset`: `last_reset() -> ResetRecord` (cause, build id, uptime, `Fault`, up to 32 stack words,
  message up to 128 bytes), `boot_count()`, `clear()`. `Fault` has one variant per architecture behind the `cortex-m`
  and `riscv` features (`std` enables both), numbers fixed by position (SW-54).
- Page `docs/std_library/ww_firmware_info.md`, test pair `tests/firmware_info`.

