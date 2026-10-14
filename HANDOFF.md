# Handoff: ww-stream-functions (API-31 stream functions and codegen)

Branch `s/14oct2026-ww-stream-functions`, linear on master (core API-30 landed at 1473db9). Task P2401#14oct-1805i.

## Done (committed)

- `ww_self`: `ApiItemKind::StreamFn { ty, delivery, args, return_ty }` replaces `PositionedStream`, `Delivery::Plain`
  first; `as_stream_fn()` / `StreamFnRef`. Plain `stream!` stays `Stream { is_up: true }` (snapshots unchanged).
- Transform: `#[stream(T, kind)] fn` (return must be none or `Result`), `#[counted]` / `#[reliable] stream!`.
- Server codegen (`api_server.rs::handle_stream_fn`, `poll_streams`; `server/stream.rs::positioned_ser_methods`):
  StreamOpen (open handler `<name>`, resume), Close → `close_<name>`, Ack/ResendFrom → `on_host_sideband`, Read →
  `status()`, `stream_<name>()` accessor, `new_stream_session()`, `_send` (counted), `_push` / `_event` (reliable).
- Client codegen: `name(args)` → `PreparedStreamOpen`, `name_resume(cursor)`, `name_status()`.
- Host runtime surface of ww-stream-host (CLT-23) is on this branch as commit 4a6a31ba (their e26bc4e2, rebased);
  agreed with them: `()` result opens to the stream itself (`With<S> = S`), they took it too.
- Scaffold, rustdoc notes, compat check / diff / api tree / py.
- Every plain stream in tests, examples, examples_mcu has open + close handlers; clients use `.call()`.
- `tests/streams_reliable` (+ `_api`): 6 tests pass; tests_common has `TestProcessEventsAsync::poll`.
- Docs: user article in `docs/api/streams.md`; `docs/evolution/rules.md` stream-kind rule; FEATURES API-31 ✅,
  API-15 superseded; CHANGELOGs. As-built design notes are in tpm's design record (Roman: no design material in the
  public repo; `docs/api/stream_reliability.md` stays as on master).

## Next

1. `just lint` again (clippy fixes in tests/streams and tests/streams_reliable just done, uncommitted at the
   moment of writing → committed with this note), then `just check` (mcu + examples_mcu) and `just test`.
2. `git rm HANDOFF.md`, commit, `tpm work ready --note "..."`.

## Gotchas

- `just test` under load: `tests/streams sideband_*_releases_seq` raced (host frees the seq after waking the caller,
  a fire-and-forget sideband right after a call gets no seq). A 5 ms settle is in the test; told ww-stream-host.
- A 16 KiB `RamRing` holds ~600 events of a 4-byte item; a producer faster than the host's acks (100 ms) fills it,
  and the dropped units only show with the next event. The tests wait for the store to drain where that matters.
- Not built: reliable streams in arrays (compile error from `poll_streams`), open args in `StreamStatus`, dynamic
  client / Python for stream functions (CLT-23), detach on link down.
- Never merge another session's branch; cherry-pick or rebase (user's rule).
