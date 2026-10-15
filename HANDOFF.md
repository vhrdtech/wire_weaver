# Handoff: IO family review round 1 (P2401#15oct-1040), additions

Branch s/15oct2026-ww-io-review1. Points 1-7 of round 1 are done and committed (one commit each, see `git log`),
just test / lint / check were green, `tpm work ready` was sent once. Roman then sent two additions (below); do them
on this branch, one commit each, then just test / lint / check, `git rm HANDOFF.md`, `tpm work ready` again.

## Next
- (a) UART RX chunks split at line idle: a gap of more than N character times ends a chunk, N a field of
  `UartConfig` (and `MonitorConfig`) with a sensible default (e.g. `idle_chars: u8`, default 2 or 3; say why), so each
  chunk is one back-to-back burst. Time of the first byte stays in the head, always set. `RxChunk::end` becomes
  `Option<Second>`: absent = first + bytes x frame bits / baud (frame bits = start + data + parity + stop), set by
  devices that measure it (2-3% baud mismatch over a long burst adds ms). Docs (ww_uart rustdoc "Time" section,
  docs/std_library/ww_uart.md "Time") say how to compute each byte's time. Update tests/io_uart (mock sets `end` on
  some chunks, leaves it off on others; test checks both), examples_mcu/usb_stm32h725ig/src/bin/uart.rs (it
  estimates, so `end: None` or keep measured end), CHANGELOG ww_uart, snapshots.
- (b) Rename `ww_ll` -> `ww_memory`: crate dir, package name, root `[workspace.dependencies]`, every user
  (`rg ww_ll`), docs page (+ `zensical.toml` nav), snapshots (`just save-snapshots --force`, remove the old
  ww_ll .ron from wire_weaver_snapshots), FEATURES mentions. Fix it: `Memory32::read` returns u8 while writes take
  u32 — make the word type consistent per trait; items documented "Optional" become `#[optional]`; errors onto
  `#[common] Common(CommonError<'i>)` like the IO family (`#[errors(..)]` on methods, `display`, ww_repr unib32, no
  self_describing). CHANGELOG (new crate name: note the rename under Breaking).

## Decisions so far (for the ready note)
- Baud rates are a new `ww_si::Baud`; every bus/bank `configure(XConfig) -> XConfig` with `io_level: Option<Volt>`;
  capabilities `io_levels` + `io_level_min/max`; `Uart::monitor` / `Spi::monitor` `#[optional]` stream fns;
  errors: Common + crate-specific only, `Error` -> `UartError` / `GpioError`, CycleSlot -> WatchSlot; watch slots u32;
  ww_can_bus untouched. API-35 filed (stream fn + borrowed error codegen bug): stream-fn handlers return `E<'static>`
  with `// TODO(API-35)`.
- Not run: `just build-docs` (breaks a running serve-docs; ask first). docs/cli.md sample output is stale.

## Gotchas
- Snapshots: `just save-snapshots --force` (nothing published yet); wire_weaver_core snapshot tests list the
  snapshots a trait resolves through (ww_error was added there).
- Don't put backticks inside double-quoted shell strings (permission prompt).
- `self_describing` enums can't hold `Hertz`/`Second` (final_structure) or `CommonError` fields.
- Float vs integer `ww_si` equality is exact: 3.3 f32 V != 3300 mV; mocks use integer mV.
- Client side sees `XErrorOwned`; `assert_eq!` can't take `{ .. }` patterns, use `matches!`.
