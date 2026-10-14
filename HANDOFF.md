# Handoff: ww-stream-reliability

Planning and coordinating session for stream positions / lost accounting / reliable delivery / backfill.

Done:
- Design `docs/api/stream_reliability.md`, approved by Roman 14 Oct with nine points, all worked in (see its Review
  section): two runs live+backfill, u64 positions in a hand-laid head, stream functions (API-15 into API-31), one
  `ww_stream` crate, read on a stream, typed cnt counters, send scheduler with priorities, sinks in units (API-34).
- FEATURES ids: API-30, API-31, API-34, SRV-15, CLT-23, TEST-19. Tasks in P2401 (#14oct-1805h..1806c).

Next (order in the doc's Implementation plan):
- piece 1 API-30 core spawned on omarchy-m1 (top); when its first commit pins the crate skeleton + StreamStore trait,
  spawn SRV-15 from its branch; after core: API-31, CLT-23, API-34 (opus); then TEST-19 and the IO family (sonnet).
- Coordinate with ww-io-traits (STD-13) and ww-errors-plan (CallError, #[errors], optional items apply to stream
  functions).

Gotchas:
- `just build-docs` not run (ask first: it breaks a running `serve-docs`).
- Head byte sizes are computed from the layout, API-30 verifies them with real bytes.
