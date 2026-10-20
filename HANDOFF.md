# HANDOFF ww-compact-ids (API-20 compact trait ids)

Plan approved by Roman 20 Oct 2026 14:57 (remove `#[ww_trait(N)]`, retry once with GlobalFull on TraitNotFound per connection, frozen-id guard as an append-only lock file, no git history).

Done (committed):
- ww_global: new wire_weaver_gid.json format, ids for all 46 public ww_stdlib traits, build.rs into OUT_DIR, ENTRIES/trait_id/crate_id/entry, assigned_ids.lock + tests (just gid-lock), README table.
- tests/global_ids: guard over every public ww_trait in ww_stdlib.
- ww_self ApiLevel::compact_id (defaulted, left out of signatures; trailing None adds no bytes, checked).
- core: levels get compact_id; server serves GlobalCompact (ww_global_compact_trait_path).
- derive: COMPACT_GID from registry, #[ww_trait(N)] is an error.
- client: GlobalFull -> compact via registry; tx keeps a full copy (FullRetry), rx hands it back on TraitNotFound, full_only per (id, major, minor).
- tests/cross_version: compact served, not found cases, pre-compact device retry.

Next:
- tap decoder / ww explain / --monitor=api: GlobalCompact by name (wire_weaver_client envelope::global_trait), P2401#20oct-1439.
- ww api check flags a changed/removed compact_id (evolution diff).
- conformance corpus GlobalCompact request case if missing.
- firmware cost on examples_mcu (flash + qemu_icount).
- docs/api/addressing.md, docs/std_library/ww_version.md, FEATURES API-20/EVO-4, CHANGELOGs, entries.md.
- just test, just lint, git rm HANDOFF.md, tpm work ready.
