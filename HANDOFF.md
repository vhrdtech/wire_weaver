# Handoff: evo-catchall (EVO-10 checker-knows-catchall)

## Done (commits on s/25oct2026-evo-catchall)
- `ww_self::ItemEnum::future_version: Option<FutureVersion>` (`#[default = None]`; `FutureVersion::Off` / `Variant { ident }`), left out of signatures (`signature.rs` clears it, like `instance_of`), so snapshots saved before keep their signatures and skipped-definition matching still works.
- `None` = model from before EVO-10: checker treats it as no catch-all (error says "save its snapshot again"); dynamic decoder reads Unsized enums leniently as before (no capability lost). `Off` = decoder fails like the derive; `Variant` = `<unknown N>` for Unsized, and for other sizes too (as the derive reads into a written catch-all).
- wire_weaver_core `convert_item_enum` sets it: written `#[future_version]` variant, else `Unknown`/`future_version = Name` for Unsized + free discriminant + no `no_future_version`, else Off. `Args` parses `no_future_version` / `future_version = X`.
- wire_weaver_evolution `check_enum`: moved check first (variant name at another discriminant on the other side = breaking, catches inserted/reordered), then unknown-to-reader passes via `check_unknown_variant` (reader catch-all + Unsized or unit variant). `compare_idents` reports changed catch-all as new `Rule::CatchAll`. Client compat uses the same code.
- Tests: evolution lib.rs (`enum_variant_appended_to_the_catch_all`, `..._without_the_catch_all`, `catch_all_changed_breaks_rust_code`), client compat `enum_variants`, ww_self value.rs `newer_variant_is_read_as_the_derive_does`. All green.
- Snapshots re-saved (`just save-snapshots`, no --force needed). Only current versions; old-version snapshots (ww_si_0_1_0 etc.) stay without the field = conservative.

## Next
- CHANGELOG entries under Unreleased: ww_self (FutureVersion, field), wire_weaver_core, wire_weaver_evolution (Rule::CatchAll, moved check), wire_weaver_client, wire_weaver_snapshots.
- FEATURES.md EVO-10 → ✅ (+ `just features-page`, and a line in tests/features_page/entries.md).
- Docs: docs/evolution/rules.md (appended variant + catch-all = compatible; moved variant), checker_tool.md if it lists rules; compat.rs module doc "names don't matter" now has the moved-variant exception; docs/std_library/ww_self.md maybe.
- Conformance coverage? `FutureVersion` is a new ww_self type, not a `ww_self::Type` variant: probably nothing; check `just test --changed`.
- `just lint`, `just test --changed`, `git rm HANDOFF.md`, `tpm work ready --note ...`, `tpm work say silicon-rev "..."` (they need ww rebuilt from this branch).

## Gotchas
- `rg` without a path in a non-tty Bash call reads stdin and hangs: always pass `.`.
- Use NEXTEST_TEST_THREADS=4 (PC is RAM-tight with other sessions).
