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

## Donguru (typec_dongle s/19oct2026-server-size)

- c85a0ec SRV-21 adopted (handlers fn, handler_model in ww/mod.rs): O3 397.8 -> 385.2 KiB, z 282.2 -> 262.1.
- 75b867a SRV-22 sizes: O3 -> 383.5, z -> 260.6.
- Measure: `CARGO_TARGET_DIR=/tmp/server-size-fw cargo build --release` (O3) and
  `CARGO_TARGET_DIR=/tmp/server-size-fw-z CARGO_PROFILE_RELEASE_OPT_LEVEL=z ...`, flash = vector_table+text+rodata+data
  from `llvm-size -A`.

## Running elsewhere

- observe-shared (opus, P2401#19oct-1434b): SRV-23 unsized ObserveTable (done there, -13.3 KiB O3 / -9.4 z on its
  baseline), SRV-24 shared observe loop in progress. Knows SRV-21/22 SHAs.
- observe-wake (top, omarchy-m1, P2401#19oct-1434c): SRV-25 event-driven observe, parked until SRV-24's SHA.
- ww-si-narrow (sibling, ready): STD-43 ww_si without i128; suggests `to_u32_in(Prefix::Micro)` in fw meter.rs.
- embassy-upstream (sibling) edits typec_dongle (embassy deps); told it which files I touch.

## Next

1. MeterReading.power drop (P2534#19oct-1340), offloaded or here.
2. When SRV-23/24/25 are ready: cherry-pick into this branch (or adopt from their branches), Donguru: server task on
   next-wake + wake signal instead of OBSERVE_TICK, meter deadband at the ADC; sizes per piece in CHANGELOG.
3. just verify in typec_dongle, fw dev + release builds, board check on omarchy-m1 (observe, settings, vbus cycle,
   USB throughput vs FW-24), or say clearly it wasn't done.

## Gotchas

- Flaky ww_stream batch::cells_tests::threaded_writer_and_reader under load: filed P2401#19oct-1446.
- `{{project-name}}_bootloader` errors in the fw build output are noise from a template dir, the build finishes.
