# Handoff: SW-62 series-store (session series-store)

Plan: ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md, piece 1.

## Done (committed)
- `shrink_wrap::series::SeriesStore` (store.rs): `type Store<const N>`, `empty_store`, `set`, `get`; impls for numbers, bool, Nibble/UNib32/UVlq, un U*/I*, UN/IN, floats, u128/i128, &str/&[u8]/RefVec<u8>, String/Vec<u8>, Option, tuples, arrays, PhantomData.
- `SeriesColumns::MAX_ITEM_BITS` / `MAX_FIXED_BITS` (default None), `series::bound` helpers (bound.rs), `Series::max_bytes(n)`; consts on every leaf impl (leaves.rs).
- Derive (shrink_wrap_derive codegen/series.rs): consts for structs/enums; `__{Name}SeriesStore` struct + `impl SeriesStore` (structs: field stores; enums: discriminant array u8/u16/u32, u32 with a #[future_version], plus each data variant's field stores). Not emitted with an opaque field, or with `series(no_store)` (args.rs).
- ww_numeric: `NumericValueStore` by hand (store.rs), `NumericValue` is `series(no_store)`.
- Tests: store_tests.rs (proptest every impl + derived), bound checks in check_golden/bytes_of/primitive check/fuzz_one (`bounded_round_trip`), `bounds_of_types`, `random_items_within_bound`; ww_numeric/tests/store.rs, ww_si/tests/store.rs (12 B/item golden with u32,i32,f32 only), run from `just test` and CI.

## Decisions (deviations from the plan, tell parent/Roman)
- Enums with data get a struct-of-arrays store (tag + variant field stores) instead of `[T; N]`: `[T; N]` needs `T: Clone` for `get`, which a derive can't require (trivially false bounds are errors). Unit enums: a tag array (same 1 B as `[T; N]`).
- `series(no_store)` opt-out: needed for NumericValue (hand store) and for a type nesting a type with an opaque field (tests/series_derive.rs `Tracks`).
- Bounds hold for every CodecPolicy (worst of all codecs), not only SMALLEST; per item e.g. u32 38 bits, f32 44, bool 5.

## Next
- Uncommitted at pause: nothing (bound tests committed with this note).
- Check the fuzz crate builds (`fuzz/`, series-type target uses type_check::fuzz_one).
- ww_si tests/series.rs goldens: add `max_bytes` asserts.
- CHANGELOGs (shrink_wrap, shrink_wrap_derive, ww_numeric, ww_si?, root [Unreleased]), FEATURES.md SW-62 ✅ + tests/features_page/entries.md line + `just features-page`, docs/serdes/series.md section (SeriesStore, bounds, series(no_store)), derive_reference.md `series(no_store)`.
- No version bumps (task says so). Then `just test`, `just lint`, `just check-mcu`; `git rm HANDOFF.md`; `tpm work ready`.
