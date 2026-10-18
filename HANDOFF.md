# Handoff: batched-observe (API-39, piece 7 of ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md)

Branch s/18oct2026-batched-observe, rebased onto master 41fc92f (ww-time-exact landed). Commits on top: parse +batched (33f23205), Samples/Batched/ObserveTable (f4806b78), server codegen (1e2c3d0b), typed host observer (f81caed4), cherry-pick of batched-streams-dynamic's 94e02cb6 (StreamFn::batch_item_ty; falls away when piece 6 lands, then `tpm work rebase`).

## Done (committed, tests green)
- `+batched` parsing, ww_self `PropertyAccess::{ReadOnly, ReadWrite} { observe, batched }`, snapshots re-saved (--force; none published), compat/diff/tree.
- ww_client_server: `ObserveMode::Samples { every_us, max_age_us }`, `Batched<'i, T>` / `BatchedOwned<T>` hand-written serdes (u4 tag; One: T; Series: align to byte + series), `BatchedOne`/`BatchedRaw` device writers.
- ww_stream::observe: Samples mode (batched slots only, max age clamped to `ObserveLimits::samples_max_age_us`), `plan_with(batch_waiting)`, `add_missed`, `SampleGate`/`GateCursor`, `batch_budget`, `batch_overhead`, `max_batched_one_len`, `MAX_UPDATES_PER_POLL`; `Outbox::remove_all_of`; unit tests.
- Server codegen (wire_weaver_core/src/codegen/server/observe.rs): ObserveBatches / split / samplers / `<Level>Sampler::sample(t, <Trait>Samples)` / ObserveBatchConsumers / ObserveBatchSource, `ObserveState<B>`, outbox per level, gates synced in observe_request, poll_properties encode phase + loop of updates, `observe_batch`/`observe_outbox` args, scaffold.
- Typed host: `LevelUpdate::times_mut` (level unwraps sample times once via DeviceTime), `Extract::Samples`, `Commander::prepare_observe_samples`, client slots `BatchedOwned<T>`, `<Trait>Observe::samples(max_age)`, latest = last sample.
- tests/observe_batched{,_api}: 8 tests (device side incl. split sampler thread on a blocking server; host per-sample with u32 wrap; level in Samples mode). Stress-run clean.

## Next
1. `ApiItemKind::Property` gets `#[default = None] batch_item_ty: Option<Type<'i>>` (the `Timed<T>` instance, None unless +batched), filled in convert_api_item_property via `timed_instance` (pub(crate) in transform/api.rs per batched-streams-dynamic). Add it where 94e02cb6 touched StreamFn: ww_self/src/visitor.rs visit_api_item_kind (Property arm), generics.rs Validator::level and Instantiator::level (search batch_item_ty), alloc.rs accessor if useful. Re-save snapshots if hashes change; a round-trip test like `batched_timed_stream_wire_item_round_trips` for a property (tests/observe_batched_api).
2. Dynamic observe (wire_weaver_client/src/client/dyn_observe.rs): LevelSchema keeps batched + batch_item_ty; decode a Batched slot (read_nib tag; 0: `des_shrink_wrap_field_dyn(ty)`; 1: align_byte, then Series of batch_item_ty via ww_self::series); DynObserver unwraps item times with its own DeviceTime, restart on Disconnected (recv via recv_event). DynResource::observe_schema in dynamic.rs.
3. Python `recv_batch()` per property for observers (wire_weaver_py/src/observe.rs), skill-tree entry per CLAUDE.md "Skill tree" (or a P2618 task).
4. Docs: docs/api/observing.md section "Every sample (+batched)" (level advanced), incl. which rates fit an MCU: ~2000 Cortex-M3 insns per item with the fixed codec (plan's piece 4 table; full-rate 21 FOC props x 25 kHz = ~1e9 insns/s does not fit any Cortex-M; a few props at a few kHz does). FEATURES API-39 status + tests/features_page/entries.md + `just features-page`. Root CHANGELOG [Unreleased] entry (memory: wire_weaver branches need one, no version bump).
5. File a SW/DER task: shrink_wrap_derive can't take a where clause naming 'i on the owned side (why Batched is hand-written).
6. Security pass (Observe Samples untrusted: clamp/validation in ObserveTable::apply, GateCursor wrapping math, outbox/push_from bounds), `just test`, `just lint`, `just check-mcu`, ask before `just build-docs`; git rm HANDOFF.md; tpm work ready.

## Decisions / gotchas
- Batched has no derive (see 5); the update layout is framework-defined and not in ww_self, so dynamic clients hard-code the Batched slot layout.
- sample() takes a values struct (`<Trait>Samples`), not getters: getters need a Context (EventOut) an ISR doesn't have. Values of properties not in Samples mode are dropped by the gate.
- A burst of more than 2 batches without a poll loses samples by design (D3), reported in `missed`.
- Host merge: any observer in Samples makes the merged mode Samples (finest grid, shortest age).
- Tests must not depend on which poll the device does first after a request (observe_batched one_values test was racy once).
