# Handoff: batched-observe (API-39, piece 7 of ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md)

## Done (committed)
- `+batched` parsing, ww_self `PropertyAccess::{ReadOnly, ReadWrite} { observe, batched }`, snapshots re-saved, compat/diff/tree.
- ww_client_server: `ObserveMode::Samples { every_us, max_age_us }`, `Batched<'i, T>` / `BatchedOwned<T>` hand-written serdes (u4 tag; One: T; Series: align to byte + series), `BatchedOne`/`BatchedRaw` device writers.
- ww_stream::observe: Samples mode (batched slots only, max age clamped to `ObserveLimits::samples_max_age_us`), `plan_with(batch_waiting)`, `add_missed`, `SampleGate`/`GateCursor`, `batch_budget`, `batch_overhead`, `max_batched_one_len`, `MAX_UPDATES_PER_POLL`; `Outbox::remove_all_of`.
- Server codegen (wire_weaver_core/src/codegen/server/observe.rs): ObserveBatches / split / samplers / `<Level>Sampler::sample(t, <Trait>Samples)` / ObserveBatchConsumers / ObserveBatchSource, `ObserveState<B>`, outbox per level, gates synced in observe_request, poll_properties encode phase + loop of updates; `observe_batch`/`observe_outbox` args; scaffold.
- tests/observe_batched{,_api}: 6 device-side tests incl. a sampler thread (split, blocking server).
- Cherry-picked ww-time-exact 287f177b, 41fc92fa (DeviceTime in wire_weaver_client::client::device_time); tpm land drops duplicates.

## Next
1. Host typed observer: client codegen slot types `BatchedOwned<T>` for +batched; `LevelUpdate` gets a times visitor so the level's `DeviceTime` unwraps every Series item t in order (on_update), observers apply the unwrapped times; `observe_<name>` of a batched property yields one `Observed` per sample (pending queue in Observer); `<Trait>Observe` `.samples(max_age)` setter.
2. Dynamic observe (wire_weaver_client/src/client/dyn_observe.rs) decode Batched slots; Python `recv_batch()` per property (wire_weaver_py/src/observe.rs).
3. Docs: docs/api page on observe (which rates fit an MCU: ~2000 Cortex-M3 insns per item fixed codec), FEATURES API-39 status + tests/features_page/entries.md, changelogs.
4. Security pass (Observe Samples untrusted), `just test`, `just lint`, `just check-mcu`, git rm HANDOFF.md, tpm work ready.

## Decisions / gotchas
- Batched has no derive: shrink_wrap_derive can't take a where clause naming 'i on the owned side (`Timed<T>: SeriesColumns<'i>`); the update layout is framework-defined and not in ww_self, so dynamic clients hard-code it. File a SW/DER task for the derive gap.
- sample() takes a values struct (`<Trait>Samples`), not getters: getters need a Context (EventOut) that an ISR doesn't have. Values of properties not in Samples mode are dropped by the gate.
- A burst of more than 2 batches without a poll loses samples by design (D3), reported in `missed`.
