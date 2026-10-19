# HANDOFF: server-size (19 Oct 2026)

Goal (Roman, 19 Oct): shrink WireWeaver's generated server and observe code, adopt it in Donguru. Design:
~/git/tpm/prj/P2534-donguru/design/fw-size-2026-10-19.md.

## Done (this branch, wire_weaver s/19oct2026-server-size)

- SRV-21 handler-model (8adaa3dd): `handler_model = "..=async, _=sync"`, per item, dotted paths, anchored regex;
  sync levels get plain fn dispatchers; `wire_weaver::handler::{sync, r#async}` make a mismatch a compile error
  (message can't name the handler: macro span). Scaffold `--handler-model`. docs/api/handlers.md.
- SRV-22 error-seq-per-item (85284f4d): fixed site numbers in `ww_client_server::err_seq` (item steps by role in step
  order 0 kind-not-taken, 1 decode, 2 handler, 3 encode, ..; deferred 32..; level 40..; request 48..), `Error::site()`,
  Display. Measured: per-item 0,1,2 counter -1.2 KiB z; fixed per-kind steps -1.6; by role -1.5/-1.7 (kept: simplest,
  same info); all-zero (lossy) would be -3.4. docs/api/errors.md#where-on-the-device-an-error-came-from.

## Also on this branch

- SRV-23 (4bf8a7f0) and SRV-24 (b55f399f) cherry-picked from observe-shared; SRV-25 from observe-wake (871db5cd,
  98f6306a, 3488a616, then the rest as one tree step), plus 24e7197b `ObserveTable::next_wake` out of line (-4.2 KiB
  O3). observe-shared and observe-wake also said ready: they hold the same commits, tpm land drops duplicates.

## Donguru (typec_dongle s/19oct2026-server-size), flash O3 / z in KiB

| step | commit | O3 | z |
|---|---|---|---|
| baseline (main 0.20.5) | | 397.8 | 282.2 |
| SRV-21 handlers fn | c85a0ec | 385.2 | 262.1 |
| SRV-22 err sites | 75b867a | 383.5 | 260.6 |
| SRV-23/24 observe | bf1673d | 363.1 | 246.4 |
| MeterReading.power gone | 55a4537 | 355.7 | 243.7 |
| SRV-25 event-driven | 9d6ccbe | 361.4 | 246.8 |

- Measure: `CARGO_TARGET_DIR=/tmp/server-size-fw cargo build --release` (O3) and
  `CARGO_TARGET_DIR=/tmp/server-size-fw-z CARGO_PROFILE_RELEASE_OPT_LEVEL=z ...`, flash = vector_table+text+rodata+data
  from `llvm-size -A`. just verify green; fw dev and release build.

## Next

- Board check on omarchy-m1 (observe incl. samples, settings, vbus cycle, USB throughput vs FW-24): session
  board-check. Not done until it reports.
- Shipped profile is still opt-level 3; the design note's z-profile switch is a separate decision.

## Gotchas

- Flaky ww_stream batch::cells_tests::threaded_writer_and_reader under load: filed P2401#19oct-1446.
- `{{project-name}}_bootloader` errors in the fw build output are noise from a template dir, the build finishes.
- tpm work spawn --from-here with two repos uses the cwd repo's SHA for both: ops#19oct-1522.
