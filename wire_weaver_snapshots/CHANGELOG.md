## Unreleased

### ⚠️ Breaking

- `exact_units`' snapshot saved again with references in the model (STD-108, EVO-48): `UnitZero`, `AmbientPressure`, `TaiScale`, `PosixScale` and `DeviceMonotonic` carry `MarkerDef::Reference`, and every origin its `ReferenceDef` whole; no other snapshot holds an origin.
- `exact_units`' snapshot saved again at its landed main 16bf14c (EVO-47), which also holds LVL-6's codecs (`CodecShape`, `CodecDef`); the snapshots that name `ww_si` types are unchanged by it.
- `ww_si` 0.4.0 (wire `w4.0`) and `ww_adc` 0.3.0 (wire `w3.0`) snapshots are new (EVO-47); every current snapshot that names a `ww_si` type (`ww_can_bus`, `ww_counters`, `ww_firmware_info`, `ww_gpio`, `ww_i2c`, `ww_indication`, `ww_logic_analyzer`, `ww_onewire`, `ww_pwm`, `ww_recorder`, `ww_signal_generator`, `ww_spi`, `ww_timebase`, `ww_uart`) saved again in place against it, as EVO-44 did. `signatures_from_before_marker_definitions`' pinned `ww_timebase::Clock` signature moves with it.
- Snapshots saved again with exact_units UNIT-9 (EVO-47): `exact_units`' snapshot holds its systems (`MarkerDef::System` for `Si`, `Natural`, `Gaussian`), Natural's and Gaussian's units and quantities, and a `Dim` that names its system; `ww_si` 0.2.0 and 0.3.0 and `ww_adc` 0.1.0 and 0.2.0 carry `SIExpr`'s new signature.
- Snapshots saved again with exact_units UNIT-8 (EVO-45): `exact_units`' snapshot lists its quantity markers (`MarkerDef::Quantity`), `Of` in place of `Phase`, and units name their quantity by id; the snapshots of `ww_si`, `ww_adc`, `ww_can_bus`, `ww_i2c`, `ww_logic_analyzer`, `ww_pwm`, `ww_signal_generator`, `ww_spi` and `ww_uart` follow the changed signatures of the units they name (`Baud` measures `SymbolRate`, `Hertz` names `Frequency`), and so do the earlier `ww_si_0_2_0.ron` and `ww_adc_0_1_0.ron`, in place (`SIExpr`, `Hertz`; neither crate is published).
- `ww_si` 0.3.0 (wire `w3.0`) and `ww_adc` 0.2.0 (wire `w2.0`) snapshots are new (EVO-44); every current snapshot that names a `ww_si` type (`ww_can_bus`, `ww_counters`, `ww_firmware_info`, `ww_gpio`, `ww_i2c`, `ww_indication`, `ww_logic_analyzer`, `ww_onewire`, `ww_pwm`, `ww_recorder`, `ww_signal_generator`, `ww_spi`, `ww_timebase`, `ww_uart`) saved again in place against it, as SW-24 did, and `ww_si_0_2_0.ron` / `ww_adc_0_1_0.ron` in place for `SIExpr`'s new signature. `signatures_from_before_marker_definitions`' pinned `ww_timebase::Clock` signature moves with it.
- Snapshots saved again with exact_units UNIT-10's `Dim` (seven exponents in halves, a sorted list of distinguished bases, described by `DimWire`'s shape) and IO-9's `UnitNames` (EVO-44): `exact_units`, `ww_si` and `ww_adc`.
- Snapshots saved again with `Magnitude`'s `Symbols { pi, ln2, ln10, mu0, g }` and 16 prime slots (exact_units NUM-10, EVO-43).
- Snapshots saved again with unit definitions as `exact_units::UnitDef` and ids on scales and origins (STD-106, EVO-35): `exact_units`' snapshot holds the new `MarkerDef`, and `ww_si` and `ww_adc` the new signature of `SIExpr`.
* `ww_numeric` 0.3.0, a new wire line w2.0 (SW-24, `NumericBaseType`/`NumericValue::INib32`, breaking: neither
  enum has a catch-all for an unknown variant). The old `ww_numeric_0_2_0.ron` stays as it was (pinned by
  `ww_si` 0.1.0's frozen pre-INT-13 snapshot); a new `ww_numeric_0_3_0.ron` carries `INib32`. Every current
  snapshot embedding `NumericBaseType`/`NumericValue` saved again against it. `signatures_from_before_marker_definitions`'s
  pinned `Clock` trait signature changed again (`Quantity` carries `NumericValue` directly, so `AnyQuantity`'s
  skipped signature moved too), same mechanism as NUM-19.
* Snapshots of the current versions saved again with `ww_si` at wire version `w2.0` (EVO-27). The `ww_board_info` 0.1.0 and 0.2.0 snapshots record `RevisionSource`'s catch-all, `Unknown(u32)`, which their source had by default (SW-47, 14 Oct 2026, before both were last saved from source): saved before EVO-10, they read as without it, so the variant `Provisional` added in 0.2.0 looked like a wire break. Re-hashed.
* Snapshots saved again for `ApiBundle::wire` and `#[since]` as a wire revision (EVO-24); the older `ww_board_info` snapshots were rewritten for the new marker form and re-hashed.
- `exact_units`' snapshot taken at db126d6: `Unbounded`'s definition changed (NUM-19), so the signatures of every type naming it changed, and `MaybeNan` carries `MarkerDef::MaybeNan` (STD-105).
- Snapshots saved again with the `ww_self::Argument` fields `default` and `flatten` (EVO-22): the signatures of traits with methods or stream functions changed with the format, so a host with these snapshots finds the stdlib traits of a device built before only through the definition cache.
- Snapshots of the current versions of `ww_global`, `ww_tap` and the `ww_stdlib` crates saved again with each enum's catch-all (EVO-10, `ww_self::ItemEnum::future_version`); signatures are unchanged. Older versions' snapshots stay without it and count as enums without a catch-all.
- The crate embeds the snapshots as shrink_wrap bytes, encoded from the RON files by the build script, and never parses RON: `all()` decodes all 36 in 1.1 ms in a release build (was 8.1 ms parsing RON), 80 ms in debug (was 633 ms); the embedded bytes are 291 KB (RON text: 756 KB). The build fails when a RON file's header lacks the `// shrink_wrap sha256: <hex>` line `ww api save` writes now, or when the file encodes to other bytes than the hash says (edited by hand, or RON does not round trip): run `just save-snapshots`. `files()` renders the text on first call from the decoded bundles with the new `render_ron()` (what `ww api save` writes), so it equals the saved files except fields added to ww_self since a file was saved (`meta: None`).
- `all()` returns `&HashMap<FullVersionOwned, &ApiBundleOwned>` (was owned bundles): `get(version)` parses only the snapshot asked for, on its first use, instead of all of them (820 KB of RON). A client connecting to a device paid for parsing every snapshot on its first connect: 150 ms and more in a debug build under load, on every short `ww` command.
- Snapshots saved again with definitions' module paths (API-46, ww_self `ItemStruct::path`): the definitions are
  the same, their signatures changed with the format, so a host with these snapshots finds the stdlib definitions of
  a device built before only through the definition cache.
- Snapshots saved again in the ww_self 0.3 format (API-43): the definitions are the same, their signatures changed
  with the format. The `ww_si` 0.1.0 snapshot holds the generic `Timed<T>`.
- Every `api_snapshots/*.ron` and `other_repos/exact_units_0_1_0.ron` saved again against `exact_units`' level-rates
  main (STD-99, LVL-3): `ww_si`'s own snapshot carries the new markers, and every crate referencing `Quantity` /
  `Prefix` / `SIExpr` as `SkippedFullVersion` carries their new signatures (unpublished crates, like STD-59's
  `Prefix::Unknown` before). The pinned `ww_timebase::Clock` signature in `signatures_from_before_marker_definitions`
  moved with it, as it did then.

### 🚀 Features

- `exact_units`' snapshot marks `Finite` with `MarkerDef::Finite` (STD-104).
- `exact_units`' snapshot holds `MuLaw`'s codec definition (`MarkerDef::Codec`, STD-109).
- `render_root_ron` renders the snapshot of one API root of a crate with several (EVO-38), with the root and its wire version in the header. The build script embeds whole-crate snapshots only and skips per-root files (`api_0_1_0.wire_v2.Thermo@w2.1.ron`), which only evolution checks read.
- The `exact_units` snapshot is saved in `other_repos/` (the crate lives in its own repository now) and copied into `api_snapshots/` like the others; it is unchanged.
- The `exact_units` snapshot holds `units::Nibble`, `Halfword` and `Doubleword` (STD-60), exact scales of `Bit` next to `Byte` and `Word`.
- The `exact_units` snapshot holds `units::Minute`, `Hour`, `Day`, `Week` and the `origins::UnixEpoch` origin (STD-84).
- The `exact_units` snapshot holds `units::Line`, `Row`, `Record` and `Entry`, counted bases beside `Item`, `Sample`, `Frame`, `Packet` (STD-85).
- Snapshots of `ww_can_bus`, `ww_dfu`, `ww_gpio`, `ww_i2c`, `ww_pin_diag`, `ww_spi` and `ww_uart` saved again with their `#[claim(exclusive)]` traits (SRV-34).
- `inline_skipped_or_cached(bundle, cache_dir)` (EVO-9): `inline_skipped` that also looks up definitions by signature
  in the definition cache.
- `ww_firmware_info`, `ww_board_info`, `ww_uid`, `ww_user_info` 0.1.0 snapshots hold the reworked device info APIs (STD-24).
- `ww_logic_analyzer` 0.1.0 and `ww_signal_generator` 0.1.0 snapshots (STD-21, STD-22).
- `ww_dfu` 0.1.0 snapshot holds the new update API (STD-12), with `StageBulk::plan` a credit sink (API-28).
- New crate: API snapshots of `ww_global` and `ww_stdlib` crates, embedded at build time. `get()` and `all()` look them
  up by crate name and version, `inline_skipped()` puts the definitions of skipped traits and types known from them
  back into a bundle, `files()` returns the embedded files as saved.
