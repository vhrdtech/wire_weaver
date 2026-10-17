# Handoff: series-dynamic (CLT-29, P2401#16oct-2039e)

## Done (commit 2e60492e)

- `ww_self::Type::Series(RefBox<Type>)` (a type of its own like `Vec`, not a generic `Instance`: `Series` is a shrink_wrap built-in with its own layout, not a definition in the types array, and a dynamic decoder special-cases it either way) and `Field::series_opaque` (`#[default = false]`) for `#[series(opaque)]`. All walkers updated (alloc, generics validate/prepare/substitute/count_nodes, visitor, compat, Python convert).
- `shrink_wrap::series::dynamic`: public column readers (`read_count`, `read_numeric`, `read_numeric128`, `read_float`, `read_run`, `read_opaque_run`, `read_struct_head`).
- `ww_self/src/series.rs`: column-by-column dynamic decoder (spec.rs shape) into a `SeriesColumn` tree; `ValueOwned::des_shrink_wrap_dyn` gives a `Vec` of items, `ValueOwned::des_series_columns_dyn` the columns (`DecodedSeries`). Restarts from tags of enums/`Option`s read before; evolvable heads; missing fields take `Field::default` (declared default, as the typed decoder); opaque fields decoded with the plain decoder.
- Budget (P2401#17oct-1125): `value.rs` `Budget` now also bounds memory (`DECODE_MAX_BYTES` = 64 MiB, `size_of::<ValueOwned>()` = 96 B per value + string/name bytes); `copies()` charges repeated values (runs, defaults). A 65535-item constant series fails fast (before reading).
- `ww_self/src/fuzz_series.rs` + fuzz target `series-dynamic`: mirror types (Kitchen with every leaf kind, opaque field, enum with data, IvSample/IvSampleV2 evolution, generic Timed) and a hand-built bundle; dyn == typed (via plain `Vec<T>` dyn decode as oracle) on the design's two 4-item golden examples, generated items, random bytes; ran 97k execs / 8 min clean. A mutation (no sibling restarts) is caught.
- Codegen: `Series`/`SeriesOwned` -> `Type::Series` (transform/ty.rs), ty_def renders them; server stream serializers use `TyPos::Def` (`'i`) because `Series<'i, T>` is invariant (two `'_` failed).
- Local bundle cache (`~/.wire_weaver/`): a file that doesn't read back in this ww_self format is rewritten (core and client), it broke `tests/cfg_variants` otherwise.
- `tests/series_api` trait renamed `Series` -> `Sequences` (clashed with the type); `readings() -> Series<Reading>`, `history() -> Series<Timed<Volt>>`, `stream!(batches: Series<Reading>)`; tests in `tests/series` (typed, introspection, dyn == typed, dynamic client, stream).
- Snapshots re-saved in the new format (RON gained `series_opaque: false`; `just save-snapshots` reports up to date, byte-identical to a fresh save).
- CHANGELOGs: ww_self, shrink_wrap, wire_weaver_core, wire_weaver_client, wire_weaver_py.

## Done in the second conversation (f7e849e3, 25172781)

- `ww api tree tests/series_api` shows `fn readings(n: u8) -> Series<Reading>` etc.
- Python `decode_columns(data)` on Method/Property/Stream (`wire_weaver_py/src/columns.rs`): dict path -> numpy array, enum tag at its path as variant names, variant fields under `path.Variant.field`, `Option` transparent, masked arrays for missing rows, `MAX_CELLS` = 2^24 bound (columns x items), numpy imported at call time (dev group has numpy). Test `test_series_columns` with golden `READINGS_6_HEX` (shared with `tests/series` `readings_golden_bytes`).
- Docs: series.md "Introspection and dynamic clients", python.md values; FEATURES.md CLT-29 ✅ + features page entry.
- Follow-ups filed: P2401#17oct-1229 (dyn Series writer), -1229b (compat of series_opaque), -1229c (codegen multi-segment generic path).
- `just test` green except two load flakes that pass alone (dfu_scenarios pit_stop_shuttle_1l_upd_step, motor_demo observe fast_forward, untouched here); `just lint`, `just check-mcu` clean.

## Next

1. Security pass running as a Fable subagent in this worktree (spawn had no room on omarchy-m1): take its fixes, re-run tests.
2. `git rebase master` (recursive-types may have landed; conflicts in ww_self/src/generics.rs are one-line `TypeOwned::Series(inner)` arms), `git rm HANDOFF.md`, `tpm work ready`.

## Gotchas

- nextest slow timeout is 3 s here: a 65535-item typed `Series` write with `CodecPolicy::SMALLEST` in debug takes > 3 s; tests use a fixed policy.
- `fuzz` needs `--cfg fuzzing`; `cargo +nightly fuzz run series-dynamic` works (use a target dir in the job tmp).
