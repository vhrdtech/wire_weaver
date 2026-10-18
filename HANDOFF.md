# Handoff: ww-conformance (TEST-23)

Done:
- FEATURES.md: API-25 to Dropped (reason, fallback), API-26 reworded (C from the ww_self model). TEST-23 still to write.
- fix(core): fixed_layout types in APIs (commit 1d7910a1).
- tests/conformance_api (types + `Conformance` trait, one property per case type), tests/conformance (corpus.json, runner):
  consumers derive, dynamic, c (C runtime via shrink_wrap_c FFI; generated fixed_layout / head_tail readers compiled with cc).

Next:
- Python consumer: wire_weaver_py/tests/test_conformance.py (fix Python unknown-variant decode in convert.rs if it fails).
- report.rs: docs/serdes/conformance_matrix.md generated, checked up to date.
- FEATURES items: TEST-23, INT-9 (TailBytes in model), INT-10 (#[default] values in model), TOOL-13 (Some(None) value form),
  SW-66 (128-bit in C runtime); INT-4 looks obsolete (bit-sized pass dynamic).
- justfile test + CI python job, AGENTS.md rule, docs page docs/serdes/conformance.md + zensical nav, CHANGELOGs (root too), features page entries.

Gotchas:
- Edit corpus.json via load/dump keeping one key per line; new case bytes "?" then CONFORMANCE=update.
- C program written to target/conformance_c/.
