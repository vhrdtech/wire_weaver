# Handoff: ww-type-aliases (API-45 walker-type-aliases)

Done (committed, `just test` 1352/1352 and `just lint` green):
- wire_weaver_core/src/transform/ty.rs: `Item::Type` in `find_user_item`/`declares_or_imports`, `convert_alias` (RHS converted in the alias' crate, params -> `Scratch::alias_args`, checked in `convert_ty_path`, `const_param_or_value`, `convert_length`; cleared in `convert_def` and for param defaults). PhantomData fields skipped in `convert_fields` (sparse: no slot taken). Unit tests `alias_tests`.
- tests/generics(_api): `type Volt = Quantity<units::Volt>`, `Sample<T>`, items 6..9 (double, voltage, timed_voltage, sample); generated client, introspection model, dynamic client. wire_weaver_cli `renders_type_aliases`.
- CHANGELOG (wire_weaver_core), FEATURES API-45 (🚧), entries.md + features page, docs/types.md#type-aliases.

Next:
- When DER-15 (ww-phantomdata, branch s/17oct2026-ww-phantomdata) has PhantomData serdes + derive: cherry-pick/rebase onto it, change tests/generics_api `Quantity<U>` to `value: i32, _unit: PhantomData<U>` with marker `pub struct Volt;` in `units` (see TODO(API-45)), update test strings (`Quantity<Volt>`), mark FEATURES API-45 ✅.
- `just build-docs` not run (ask the user first, it breaks a running serve-docs).
- `git rm HANDOFF.md`, then `tpm work ready`.

Gotchas: an alias named like a built-in (`type Result<T> = ...`) still hits the built-in match first.
