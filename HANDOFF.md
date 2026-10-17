# Handoff: generic-types (API-43 generic-types-in-apis)

Task P2401#17oct-1011. FEATURES id is **API-43** (the brief said API-42, which was already taken by credit-sink positions). Roman's calls (17 Oct 2026): generics in ww_self (not per-use monomorphized structs); then FULL generics, break formats freely (generic enums, const params, where clauses), bound everything on the untrusted introspection path.

## Done (all committed on s/17oct2026-generic-types, `just lint` clean, 1298 tests pass, `just test-py` 29 pass)

- Derive layer (child session generic-derive, cherry-picked 4f4efa7e, 441b6aa2): generic enums, const params, where clauses, DER-14 docs.
- ww_self 0.3.0 (370705ed): `generics` on ItemStruct/ItemEnum, `Type::Param`, `Type::Instance { type_idx, args }`, `GenericArg::{Type, Const, ConstParam}`, `Length::{Fixed, Param}` for Array/MaxLen. `ww_self::generics::{validate, instantiate, prepare, substitute, params_human_name}` with MAX_DEPTH 96, MAX_INSTANCES 4096, MAX_NODES 1<<20. value.rs decode/default bounded by a value budget (64 per byte + 65536) and a depth limit. human_definition bounded (and its `{{` bug fixed).
- Design decision (deviation from the brief's wording, say it in the report): dynamic decoders don't thread an environment; `prepare` instantiates once per bundle (concrete definitions appended, named `Timed<Ampere>`), so value.rs, compat, layout, CLI and Python work unchanged on concrete types. One bounded, auditable place instead of env threading through ~10 walkers.
- wire_weaver_core: transform/ty.rs resolves generic user types (scope in `Scratch::generic_scope`, `find_user_item`, `convert_user_ty`, `convert_def`, `convert_named_def`), const args, defaults; ty_def.rs emits `Timed<Ampere>` / `TaggedOwned<..>`; snapshots include generic defs; snapshot remap/used visitors handle Instance.
- wire_weaver_client: `inline_known` -> `prepare` (malformed = error), `IntrospectBundle::from_sent` returns Result, Promise OnDone returns Result; compat: instances of unknown defs compared by def+args, `check_generics`, Param vs Param; layout leaf cap; evolution `compare` ignores ww_self_version.
- CLI: `ww api tree -t` shows `<T, const N: u64>`; `ww api save` "reformatted" when only the format changed. Python `load_api` prepares.
- Snapshots: migrated RON + `just save-snapshots --force` (signatures change with the format; definitions unchanged). ww_si snapshot has generic `Timed<T>`.
- tests/generics + tests/generics_api (48b3cb36 era): generated client, introspection (Timed left out and put back), dynamic client, same bytes.
- Fuzz: `ww_self::fuzz_bundle` + `fuzz/src/introspection.rs`; 1.6M runs / 5 min clean.
- Docs: docs/types.md#generic-types-in-an-api, docs/evolution/rules.md#generic-types, derive pages, ww_si page; FEATURES API-43 + entries.md + features page; CHANGELOGs (ww_self, core, client, cli, py, snapshots, ww_si).

## Open

1. Child session **decode-depth** (top model, on omarchy-m1, branch s/17oct2026-decode-depth from origin/master) fixes P2401#17oct-1103 (+security medium, pre-existing): shrink_wrap owned decode of recursive types has no depth limit, a ~38 KB introspection blob with 10k nested Type::Vec aborts the host with a stack overflow before `prepare` runs. When it says ready with a SHA: fetch its branch (it's on another PC: `git fetch` from its remote/ask how tpm moves it) and cherry-pick; then extend `ww_self::fuzz_bundle` with a deep-nesting case if the child didn't.
2. Security pass: child session **generic-sec** (spawned `--model top --from-here` from c2ffa298, branch s/17oct2026-generic-sec, worktree ~/git/.wt/generic-sec on this PC; top resolved to opus). It fixes findings on its branch and reports SHAs: cherry-pick them, rerun `just test` / `just lint`.
3. Optional: `just build-docs` only after asking the user (may break a running serve-docs).
4. Before ready: `git rm HANDOFF.md`, commit, `tpm work ready --note "..."`.

## Gotchas

- syn 3: `TypeParam.default` / `ConstParam.default` are `Option<(Eq, T)>`.
- RON snapshots: `ww api save` compares against the old file, so a format change needs the files migrated first (script was a one-off regex: `generics: []` after `ident`, `Fixed(n)` for array/MaxLen lengths).
- Non-lifetime derived types need `borrowed, owned(..)` to be usable by no_std servers (no `XOwned` type for them).
