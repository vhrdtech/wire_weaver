# Handoff: STD-37 si-dynamic-units (session ww-si-dynamic)

## Done (committed on s/17oct2026-ww-si-dynamic)

- ww_si: `Dim` is a `sized` derive of seven `I4` (28 bits), `Dim::new` (const, panics out of -8..=7), `checked_new`, `exps`, `checked_mul/div/pow`; `Scale` on the wire (`ww_repr = u4`, `no_future_version`); `UnitExpr::Pow`.
- `src/expr.rs`: one generic walk (`Tree` trait, `Node`) for symbol, name, dim, factor, `same`, `write_unit`, used by `UnitExpr`, `SIExpr`, `&SIExprOwned`; bounded by `MAX_UNIT_DEPTH` = 32.
- `src/dynamic.rs`: `SIExpr<'i>` (Named / Mul / Div / Pow, RefBox children), `Unit::SI_EXPR` (default `SIExpr::named(EXPR)`, `Per` / `Times` build their tree in const), `SIExpr::of::<U>()`, `known()`, `== UnitExpr`, owned conversions; `AnyQuantity<'i>` (+ Owned) with `From<Quantity<U>>`, `to_quantity`, `to_si_f64`, Display / Debug / defmt.
- ww_adc `ChannelInfo::unit: SIExpr<'i>`; `BaseUnit`, `SIExp`, `INib8P3` removed; tests/io_adc checks the units on the client.
- Snapshots ww_si 0.2.0 and ww_adc 0.1.0 re-saved with `--force` (both never published).
- Docs `docs/std_library/ww_si.md#units-known-only-at-run-time` with real bytes (tests/dynamic.rs pins them), CHANGELOGs, FEATURES STD-37 🚧, features page.
- `just check` and `just lint` green; `just test` not run in full yet (RAM: run with NEXTEST_TEST_THREADS=5).

## Next

1. Wait for CLT-31 (s/17oct2026-ww-si-clients) on master (parent says when), then `tpm work rebase`.
2. CLT-31's `DynUnit::from_expr` and `DefmtName`-like matches on `UnitExpr` need the new `Pow` arm; its `Dim` uses may need `exps()` / `Dim::new` (fields are `I4` now).
3. Client: recognize `ww_si::AnyQuantity` values in dynamic values (wire_weaver_client `quantity.rs`), build the unit from the `SIExpr` value (Named → `by_name` when `known`, else a unit with dim and scale), show it through the same `DynQuantity` path; `ww` and Python `ww.Quantity` too.
4. `AnyQuantity` property in `tests/quantities_api` (CLT-31's test pair), test through in-process transport, typed and dynamic client.
5. FEATURES STD-37 ✅, changelogs (wire_weaver_client, cli, py), `just test`, `git rm HANDOFF.md`, `tpm work ready`.

## Decisions

- `Dim` fields are `I4`, not `i8`: the derive has no per-field bit width, `I4` is what the walker knows (`IB(4)`).
- `Scale` and `SIExpr` are `no_future_version`: a reader can't use a scale or unit kind it doesn't know.
- `Unit::SI_EXPR` instead of a run-time UnitExpr → SIExpr builder: a borrowed `SIExpr` needs `'static` children, only const can give them without alloc.
- `docs/` not built (`just build-docs` would break a running `serve-docs`): ask before running it.
