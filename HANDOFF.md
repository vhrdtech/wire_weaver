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

## Next

1. Disk: /home was full (283G in ~/.cache/cargo-build); parent told. Builds fail at link until space is freed.
2. CLI: `ww` already prints Series values as lists of items through `value_fmt::fmt_value` (Vec); `human_name` gives `Series<Reading>`. Check `cargo ww api tree tests/series_api` shows it (was blocked by disk), maybe a value_fmt test.
3. Python (`wire_weaver_py`): Series converts to a list of items both ways already (encode then fails: dynamic Series writer not supported). Add `decode_columns(data)` on resources whose type is `Series<T>` (property/method return/stream): dict path -> numpy array via runtime `import numpy` + `numpy.frombuffer` (no Rust numpy crate), masked arrays (`numpy.ma`) for leaves under a variant/`Option`; paths `v.prefix`, `v.value.U16.0`, tag columns as variant names. Python test in `wire_weaver_py/tests/test_offline.py` using `tests/series_api` (needs a `load_api` path) or `tests/dynamic_api`. Docs `docs/python.md`.
4. Docs: `docs/serdes/series.md` (or whichever page piece 4 wrote) gets a "dynamic decoding / introspection" section; FEATURES.md CLT-29 item status + `tests/features_page/entries.md` line + `just features-page`.
5. Follow-ups to file (P2401 tasks): dynamic Series writer (TODO(CLT-29) in value.rs `ser_inner`); compat check ignoring a changed `series_opaque` (TODO(CLT-29) in compat.rs); codegen resolves generic args of a multi-segment path (`shrink_wrap::Series<'i, Reading>`) in the path's crate, not the current one (pre-existing, found here).
6. `just test`, `just lint`, `just check-mcu` (mcu/Cargo.lock if ww_self changes), security pass (on top; this decodes untrusted device data), `git rebase master` (recursive-types session touches `ww_self/src/generics.rs` validate: my edits there are one-line `| TypeOwned::Series(inner)` arms), `git rm HANDOFF.md`, `tpm work ready`.

## Gotchas

- nextest slow timeout is 3 s here: a 65535-item typed `Series` write with `CodecPolicy::SMALLEST` in debug takes > 3 s; tests use a fixed policy.
- `fuzz` needs `--cfg fuzzing`; `cargo +nightly fuzz run series-dynamic` works (use a target dir in the job tmp).
