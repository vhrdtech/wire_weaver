## Unreleased

### ⚠️ Breaking

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
