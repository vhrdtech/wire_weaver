# Handoff: credit-sink-resume (API-34, planning + piece 1)

Plan and as-built notes: `~/git/tpm/prj/P2401-wireweaver/design/sink-sessions.md`.

Done (piece 1, protocol core):
- `ww_stream::sink`: `SinkCredit` (device) and `SinkTx` (host, `alloc` feature), unit positions modulo 2^28,
  grants in bytes, host-named sessions, Close for writes while not open, capacity in the open answer.
- `ww_client_server`: `RequestKind::SinkWrite`, `StreamOpened::capacity`, `Ack`/`Query` with `backfill` first.
- Tests: `ww_stream/src/tests/sink.rs` (ported from `ww_client_server::credit` and the client's `CreditTx`),
  `ww_stream/src/tests/sink_sim.rs` (seeded lossy sim, 20 000 seeds green).

Running elsewhere:
- sink-switch (P2401#19oct-1710, opus, branch s/19oct2026-sink-switch from 2f025b37): codegen, client, Python,
  ww_dfu, tests, examples_mcu, typec_dongle onto `ww_stream::sink`; removes the old `Credit`/`CreditQuery`/`Resend`,
  `CreditWrite`, `ww_client_server::credit`; docs. Told to cherry-pick 1e5bb87e and 5dd416ba.

Next:
- After sink-switch is ready: spawn sink-resume (P2401#19oct-1711, opus) from its tip.
- After sink-resume: TEST-26 on omarchy-m1 (P2401#19oct-1711b, sonnet, `--on omarchy-m1`).

Gate on 6c548128: just lint green; just test 1673/1674, the one timeout is batch::cells_tests::threaded_writer_and_reader
under load (0.05 s alone, untouched here), filed as P2401#19oct-1738.
SinkWrite is two variants (SinkWrite / SinkWriteUnits, `RequestKind::sink_write`), see 6c548128.

Decisions: see the design note; plain sinks keep sideband `Open` as a user sideband (API-53 later).
Gotcha: the device's Ack after an open is sent before the answer on the wire in sink-switch's codegen; `SinkTx`
drops credit events before `opened()`, so the host buffers sideband until the answer.
