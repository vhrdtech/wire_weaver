# Handoff: ww-stream-reliability

Planning session for stream positions / lost accounting / reliable delivery / backfill.

Done:
- Design doc `docs/api/stream_reliability.md` (in zensical nav, linked from `docs/api/streams.md`), wire costs
  measured with a scratch crate (not in repo).
- FEATURES ids claimed and added: API-30 counted-reliable-streams, API-31 typed-stream-open, SRV-15 stream-stores,
  CLT-23 host-stream-positions, TEST-19 lossy-test-medium.
- Tasks filed in P2401 (see `tpm show P2401`), not spawned: waiting for Roman's review of decisions R1-R7.

Next:
- Roman answers R1-R7 (in chat); update the doc to the decisions, then spawn pieces 1 (top) → 2+3 (opus) and 4 (top)
  → 5, 6 (sonnet). Piece 6 coordinates with session ww-io-traits (STD-13).

Gotchas:
- `just build-docs` not run (ask first: it breaks a running `serve-docs`).
- ww-io-traits already removed seq/lost/t from item types and uses sideband Open/Close + `configure_stream`.
