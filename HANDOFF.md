# Handoff: recursive types (API-44)

Task P2401#17oct-1138: self-referential types in device APIs.

## Done (committed on s/17oct2026-recursive-types)

- `wire_weaver_core` transform: a self-reference reserves the definition's index (`convert_def` / `refer_back` in `transform/ty.rs`, `Scratch::types_in_progress` / `recursive_types`); cycles through a generic definition are an error naming the cycle. Unit tests `transform::ty::recursive_tests`.
- `ww_self::generics::validate`: Tarjan SCCs; a cycle may not contain a generic definition; depth of a cycle = sum of its members' local depths (a walk with an on-path set goes through each once); `check_indirection`: every cycle must go through `Box` or `Vec` (generic params tracked: `W<T> { Vec<T> }` guards its argument). `instantiate` works on a clone of each pending def (the def can name itself in `Timed<Expr>`).
- Walkers: `is_lifetime` (seen set), `human_definition` (open set, names inside itself), `ValueOwned::default` (first enum variant without `RefersBack`), `inline::Import` non-user entries copied once, `compat::TypeCheck` keyed on (writer def ptr, reader def ptr) of resolved struct/enum + `MAX_CHECK_DEPTH = 4 * MAX_DEPTH`, Python `to_value_at` depth bound = `ww_self::MAX_DECODE_DEPTH`.
- Dynamic decoder: `MAX_DECODE_DEPTH` public, `4 * MAX_DEPTH` = 384; `des_ty`'s bulky arms moved to `#[inline(never)]` helpers (`des_items`, `des_enum`, `des_numbers`): des_ty frame was 6.3 KB in debug, a 64-level list overflowed 2 MiB.
- End to end: `tests/recursive_api` (`Expr` through RefBox, `Tree` through RefVec) + `tests/recursive` (generated server/client, property, stream, introspection, dynamic client, too-deep values), `ww` render test `renders_recursive_types`, Python `test_recursive_types`.
- Fuzz: `fuzz_bundle` recursive generator mode (mode byte bit 1), unit fuzz on a 2 MiB thread, `gen_bundle` public behind ww_self `fuzz` feature; `compat::tests::generated_bundles_in_pairs`.
- Docs (`docs/types.md#self-referential-types-refbox`, cli_reference limitation removed), FEATURES API-44, CHANGELOGs.

## Next

1. `just test` / `just lint` results (run at the end of step 1).
2. Security pass on the walkers on the top model (spawned child session, see below), cherry-pick its fixes.
3. `git rm HANDOFF.md`, `tpm work ready`.

## Decisions / gotchas

- "Cycles through Instance" is read as: a cycle that contains a generic definition. A non-generic type using itself as an argument (`Vec<Timed<Tree>>`) is fine.
- Cycles with no Box/Vec are rejected (as Rust does), not just tolerated: walkers then never see a type without a finite value.
- `ww_si::SIExpr` is not derived (no `derive_shrink_wrap`), left alone: deriving it changes the ww_si snapshot.
- The dynamic decoder allows ~95 list levels (`Option<Box<L>>`), ~128 `Neg` levels; the device's reader 128 unsized levels.
