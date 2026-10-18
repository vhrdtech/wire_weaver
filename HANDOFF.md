# Handoff: shared-clock (I-2 / API-49), session on omarchy-m1, branch s/18oct2026-shared-clock

Brief: implement I-2 (one time column per observe level, `Batching::SharedClock`) and measure it. Design note: ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md ("Shared clock, measured", "Shared clock for batched observe: plan"; tpm-repo edit uncommitted there, leave it for the user, but its Wire section is outdated, see below).

## Done

- Step 1 (measured): RawPlain, benches, numbers in the design note, parent told.
- Step 2 (implemented, tests green: `cargo nextest run --workspace` 1493/1493, `just lint` clean):
  - Device runtime (ww_stream): level rows, one batcher per level, column-wise encode, level outbox, level-wide Samples settings.
  - Server codegen: `<Trait>Row`, one gate per level, poll offers the oldest level batch whole, `Selection::is_dropped`.
  - **Changed from the plan:** the time column is NOT a slot at `item_count` (that moved whenever a resource was added: an additive change would break the wire). It is `EventKind::PropertyUpdate::sample_times: Option<Series<Second>>` (after `missed`), written by `ser_property_update_event(.., sample_times: Option<&[u8]>, data)`. Consequences: ww_self has no `sample_time_ty`, a `+batched` property needs no `ww_si` dependency, no `sample_times` name clash, host level unwraps it like `t`, `Observed::sample_times` for level observers, `LevelUpdate::update_latest(.., sample_times)`, no `times_mut`/`set_times`/`init_latest`/`DynLatest`.
  - Behaviour note: rows carry every value, so a property switched into Samples also gets the rows not sent yet (≤ one batch), each at its time (test `validated_and_switched_off`, documented).
  - Tests: tests/observe_batched (12, new: level_wide_samples_settings, column_dropped_from_a_waiting_batch, small_buffer_seals_smaller_batches), ww_client_server update_event_in_place (byte-equal to derive, with/without times), core batched_property_shared_clock.
  - Docs: docs/api/observing.md (Every sample section, MCU rates table with current numbers, wire format), docs/evolution/rules.md. CHANGELOGs Unreleased (ww_client_server, ww_self, ww_stream, wire_weaver_core, _client, _cli, _py). FEATURES API-49 ✅ (+ entries.md, features-page). Snapshots unchanged (`just save-snapshots --force`: all up to date).

## Next

1. Gates: `just check-mcu`, `just check-examples-mcu` (running), `just build-docs` (allowed).
2. Update the design note's plan section (Wire: sample_times in the event head) — tpm repo, uncommitted for the user.
3. Tell batching-docs session (gpd-omarchy) the final shape; `tpm work say parent` summary.
4. `git rm HANDOFF.md`, `tpm work ready`.

## Gotchas

- QEMU runs only on gpd-omarchy. `probe-rs run` of H725 bins idles after "done".
- `#[derive_shrink_wrap(series)]` inside `macro_rules!` breaks (hygiene), filed P2401#18oct-1150.
