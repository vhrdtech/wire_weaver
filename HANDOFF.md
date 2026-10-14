# Handoff: ww-stream-reliability

Coordinating session for stream positions / lost accounting / reliable delivery / backfill
(design record: tpm prj/P2401-wireweaver/design/stream_reliability.md; docs/api/stream_reliability.md is now
the user-facing article "Stream delivery", no review traces: wire_weaver is public).

State:
- API-30 protocol core: landed (1473db9), session ww-stream-core closed.
- SRV-15 LogStore: ww-stream-logstore, woken to `git rebase master` and start (was blocked on core).
- API-31 stream functions + codegen: ww-stream-functions (opus), owns wire_weaver_core/derive, server, ww_self.
- CLT-23 host runtime: ww-stream-host (opus), owns wire_weaver_client; commits the runtime surface first for API-31.
- Next: API-34 sink-sessions (task P2401#14oct-2053, opus) once API-31 is in (both edit stream codegen); then TEST-19
  (P2401#14oct-1806b, sonnet) after API-31 + CLT-23, and the IO family (P2401#14oct-1806c, sonnet) with ww-io-traits.

This branch: the article rewrite of docs/api/stream_reliability.md, its nav entry and a pointer from streams.md.
Keep the article in step with what lands (API-31 syntax, CLT-23 host API, SRV-15 LogStore).

Decisions taken here: units in the head (`L T X U nnn p`, from ww-stream-core); no compatibility shims (Roman's
policy); logstore waited for core to land rather than having its branch moved (guard).
