# Handoff: ww-si-core (STD-38 ww_si 0.2 core)

Design: `~/git/tpm/prj/P2401-wireweaver/design/ww_si_generic.md` (all recommendations approved 17 Oct 2026).

## Done (committed on s/17oct2026-ww-si-core)

- ww_si 0.2 core: `src/unit.rs` (Dim, Scale, UnitExpr, Unit, UnitMul/UnitDiv), `src/units.rs` (literal markers, `units!` table for the `impl Unit`s and `ALL`, Per/Times, `by_name`), `src/quantity.rs` (generic methods, PartialEq/Ord, Display/Debug/defmt), `src/algebra.rs` (operators through Scale, `products!` table with const SI checks), `src/lib.rs` (Prefix + `symbol()`, `Quantity<U: Unit>`, one alias per unit, Ratio percent/per_mille/ppm, DeciBel/DeciBelmW plain).
- Version bumped to 0.2.0 (ww_si Cargo.toml, root `[workspace.dependencies]`, Cargo.lock, mcu/Cargo.lock).
- ww_numeric: `NumericBaseType::from_i128` and `from_f32_saturating`.
- Tests: `tests/quantities.rs` (golden bytes = 0.1, algebra, display, quantize, with_prefix, to_si), `tests/series.rs` (series.md 14 B example, Timed series, 32 x `Timed<Volt>` = 22 B taken from 0.1 on master), compile_fail doctests in the `Quantity` rustdoc. Pass with default features and with `--no-default-features --features electrical,f32`.
- Call sites: struct literals → `from_parts`, named constructors / `quantity!` → prefix constructors, across ww_stream, wire_weaver_client, ww_client_server, ww_report, tests/*, motor_demo, examples_mcu/*, docs.
- Docs `docs/std_library/ww_si.md` rewritten; CHANGELOG ww_si + ww_numeric; FEATURES STD-38 (🚧) + features page entry.

## Decisions

- `BaseUnit`, `SIExp`, `INib8P3` kept unchanged (parent, 17 Oct): `ww_adc::ChannelInfo::unit` uses BaseUnit; STD-37 replaces all three. `Unit<'i>`, dynamic `Quantity<'i>`, `SIExpr` stub removed.
- `UnitExpr::Named` carries `dim` too (for by_name / dynamic clients). `Unit::EXPR` is `&'static UnitExpr`.
- Operators: `Mul`/`Div` factor `k = fa·fb/fo` computed in an inline `const` block; affine scales (°C) are rejected in table lines by a const assert; `U * Ratio` uses factors only, so °C · 0.5 scales the °C value.
- Operators keep `Prefix::Unit` and `F32`/`F64` storage (D4), also `* f32`.
- Snapshot loader not touched: API-46 (session ww-codegen-paths) makes it see `pub mod units` and key entries by path.

## Next

1. Wait for API-46 on master (parent pings), `tpm work rebase`.
2. `cargo check --keep-going --workspace --all-targets`: the remaining errors (`Quantity<Quantity<..>>`, codegen naming the marker as the alias) should be gone; fix what is left in motor_demo, tests/generics, series, cfg_variants, counters, indication, io_can*, io_i2c, io_timebase, io_uart, examples/uart, ww_report, examples_mcu/usb_stm32h725ig uart.
3. `wire_weaver_client` `compat::tests::other_revision::*` (3 tests) use `ww_si::Volt` as a skipped struct with fields (0.1 model); in 0.2 `Volt` is a field-less marker and there's no 0.2.0 snapshot yet, so they fail. After the snapshot, move their fixture to `Quantity` (the generic def with `prefix` / `value`) or the marker, whatever keeps what they test (a skipped definition from another revision, from the cache).
4. `just save-snapshots` (ww_si 0.2.0 snapshot; other crates' snapshots re-saved only if they must, their migration is piece 4), the `wire_weaver_core` snapshot test.
5. FEATURES STD-38 → ✅, features page; `just test`, `just lint`, `just check-mcu`, `just check-examples-mcu`; `git rm HANDOFF.md`; `tpm work ready`.

## Gotchas

- `cargo check --workspace` stops early on errors: use `--keep-going` to see every crate.
- A slice of quantities returned as `'static` needs `&const { [Volt::from_parts(..)] }` (prefix constructors aren't const).
- Helper scripts used for the rewrites were one-off (job tmp), not in the repo.
