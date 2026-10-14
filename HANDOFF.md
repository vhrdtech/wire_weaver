# Handoff: ww-stream-host (CLT-23 host-stream-positions)

## Done (all committed, was `tpm work ready` before master moved)

- Public runtime surface: `Commander::prepare_stream_open::<R, S>(path, Option<ww_self::Delivery>, args)`,
  `prepare_stream_resume::<T>(path, Delivery, cursor)`, `PreparedStreamOpen`, `StreamOpenResult` (`()` -> handle,
  `Result<A,E>` -> `Result<(A, handle), E>`), `StreamHandle` (`Stream<T>` | `PositionedStream<T>`).
- `PositionedStream<T>` / `StreamEvent<T>` / `StreamStats` / `AckMode` in `wire_weaver_client/src/client/positioned.rs`;
  shared state `Shared` with the `ww_stream::rx::Receiver`, run queues, split.
- Event loop: `Command::OpenStream` + `StreamOpenHook` (dispatcher runs `opened()` when the answer arrives),
  `StreamItem` / `Query` routed in `rx_dispatcher.rs`, Ack/ResendFrom polled every 20 ms (`ToTx::Send`).
- Bounded queues for all plain streams (`event_loop/event_queue.rs`, `RawStreamEvent::Dropped`,
  `ClientConfig::stream_queue_bytes`). Raw `StreamEvent` renamed `RawStreamEvent`.
- `disconnect().keep_streams()` keeps positioned streams (`ToRx::KeepStreams`), `Commander::resume_streams()`.
- `DynPositionedStream` (`client/dyn_positioned.rs`), Python `PositionedStream` (`wire_weaver_py/src/positioned.rs`).
- Tx waits for a free seq instead of failing (CLT-18 fix, test `request_waits_for_a_free_seq`).
- Tests: `wire_weaver_client/src/event_loop/stream_e2e_tests.rs` (S4 x3, S6, reconnect) against a hand-written device.
- Docs: `docs/api/stream_reliability.md` "Receiving on the host", `docs/python.md`; FEATURES CLT-23 🚧; CHANGELOGs.

## Next

1. `git rebase master` (plain). master is at 901ffae+ with ww-stream-functions (API-31) and ww-stream-logstore
   landed. API-31 merged an early copy of my surface commit and changed things on top: `ww_self::Delivery` has a
   `Plain` variant (they patched `Shared::new` with `Plain => unreachable!`), `ApiItemKind::PositionedStream`
   became `StreamFn { ty, delivery, args, return_ty }`, `Stream::open` removed, `()` -> `With<S> = S` (same as
   mine). Expect conflicts in `wire_weaver_client/src/client/*` (positioned.rs, prepared_stream_open.rs,
   dynamic.rs, stream.rs), CHANGELOG, FEATURES; keep both sides' intent.
2. After the rebase: `DynResource::delivery()` / `positioned_stream()` and the Python tree must use `StreamFn`;
   add stream function args (`args: &[ValueOwned]`) to `positioned_stream` (TODO(API-31) there).
3. In `tests/streams`, drop the 5 ms settle in `sideband_*_releases_seq` (API-31 put it in; my seq fix makes it
   unneeded).
4. `just lint`, `just test`, `just check`; `git rm HANDOFF.md`; `tpm work ready --note "..."`.

## Gotchas

- No design/review notes in the repo (Roman's rule); docs pages describe the feature as it is. Decisions are in
  tpm `prj/P2401-wireweaver/design/stream_reliability.md`.
- Host sidebands must keep a seq (device codegen replies to sideband requests with seq != 0, tests rely on it).
- Reliable queue bound = window plus the device's 4 KiB initial grant before the first Ack (test asserts that).
- Positioned streams only on absolute paths (events carry the absolute path).
- Session commits never bump versions.
