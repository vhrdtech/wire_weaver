# Handoff: series-dyn-write

Done (committed):
- 1b46b21a CLT-29 write side: shrink_wrap::series::dynamic writers, ww_self series_write.rs (ValueOwned::ser_series_dyn, ser_shrink_wrap_dyn of Series), differential tests vs typed writer for every policy, DynClient + Python write of `log: Series<Reading>`.
- c2345db0 removal of Delta/DeltaOfDelta/XorFloat in code (shrink_wrap, ww_self, core, client, py, ww_logic_analyzer, tests, snapshots), changelogs.

Next:
- docs: special_types.md section, showcase.md section (byte-verified via scratch example), wire_format.md (keep codes, as Series internals), series.md links, types.md, python.md, c.md, derive.md, evolution/rules.md, std_library/ww_self.md, ww_logic_analyzer.md, api/stream_reliability.md; FEATURES SW-31/SW-32/SW-50/SW-28 superseded; tests/features_page entries; `just features-page`.
- gates: just test, lint, check-mcu, check-examples-mcu, build-docs; git rm HANDOFF.md; tpm work ready.

Measurements (ww_logic_analyzer edges, 128 edges, bytes DoD / Series SMALLEST / DeltaPacked, host ns per encode): steady clock 23/8/82 (508/2805/860 ns), +-1 jitter 129/76/130, UART 151/150/150, sparse 398/300/304, full-range random 586/427/431, 5 edges 15/11/14. max_bytes(128)=610.

Gotchas: an opaque field's change is by value equality (as the typed writer: -0.0 after 0.0 is no change, so lossy) — dynamic writer decodes each opaque value to compare like typed. Nested evolvable struct lengths memoized per start bit (typed writer is exponential in depth of nested evolvable structs; not fixed here).
