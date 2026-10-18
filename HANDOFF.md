# Handoff: shared-clock (I-2), session on omarchy-m1, branch s/18oct2026-shared-clock

Brief: implement I-2 (one time column per observe level, `Batching::SharedClock`) and measure it. Design note: ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md, sections "After SW-63", "Shared clock, measured" and "Shared clock for batched observe: plan" (read the plan section first, it is the spec; the tpm-repo edit is uncommitted there, leave it for the user).

## Done

- Step 1 (measured, committed, parent told): `shrink_wrap::series::RawPlain`; benches `examples_mcu/mcu_qemu/src/{shared_clock,foc_signals}.rs`, bins `qemu-shared-clock-{delta-packed,raw-plain}` (M3, M4F), `stm32h725ig-shared-clock-cycles`; `qemu_icount` streams the trace (no file) and stops at `--max-insns`/`--timeout`. H725: -36 % bytes, -52 % cycles (DeltaPacked); QEMU -46 % insns. The row-of-Options single series was measured and dropped (7x cycles).
- Roman OK'd (via parent): SharedClock is the default and ONLY `+batched` observe form (piece 7's per-property Timed form goes); a level's `Samples` entries share every_us (finest) and max_age_us (shortest), reported in the grant.
- Step 2, committed in 425249e2 (compiles on its own, tests green: `cargo nextest run -p ww_stream -p ww_client_server`):
  - ww_stream::batch: Batcher core needs only `SeriesStore`; `Bound`; `LevelRow` trait (level.rs: `COLUMNS`, `TIME_BITS`, `COLUMN_BITS` via `column_bits::<C>()`, `encode_column`, `bound`, `max_bytes`), `Batcher::new_rows`, `Consumer::poll_rows(now, age, budget, columns)`, `Consumer::encode_rows`, `Consumer::policy`, `Scratch::encode_column`. Outbox rewritten for level batches: entries `(k, items, len)`, a batch = TIME entry (k = u16::MAX) + column entries (k = table slot); `push_rows_from(consumer, columns, slots, scratch)`, `batches()`, `oldest()` -> `LevelBatch { rows, time, columns(), column(k) }`, `remove_oldest()` (returns samples), `retain_columns(keep)`, `clear()`. check.rs implements LevelRow for its `Item` tuple, fuzz check exercises level batches.
  - ww_stream::observe: apply() unifies Samples modes; `level_samples()`; `LevelDesc::time_slot: Option<u32>` (header_bound counts it); `Selection::offer_batch(&[k], size)` all-or-none + time slot, `has_time()`, `present()` appends time_slot; `batch_budget(desc, overhead, buf_len, outbox_bytes, columns)` (outbox bytes whole now, it subtracts entry headers itself).
  - ww_client_server: `Batched::{One, Samples(Series<T>)}` (tag 1), `BatchedRaw`, new `SeriesRaw` (sample_times slot), `BatchedOwned::{One, Samples(SeriesOwned<T>)}`, `into_samples(t, sample_times: Option<&[Second]>) -> Result<..>` (count mismatch = `Error::MalformedSeries`); `times_mut` removed.
  - ww_self: `Batching::SharedClock`; `ApiItemKind::Property { .., #[default] batched: Option<Batching>, #[default] sample_time_ty: Option<Type> }` (replaces `batch_item_ty`); `PropertyAccess::{ReadOnly, ReadWrite} { observe }` (bool `batched` gone: PropertyAccess must stay self_describing, Option<Batching> isn't). Streams with SharedClock are a compile/bail error.
  - wire_weaver_core transform: `PropertyMacroArgs.batched: bool`; model gets `batched: Some(SharedClock)` and `sample_time_ty = ww_si::Second` (`ww_si_second()` in transform/api.rs); codegen api_client / client observe / server observe read `batched.is_some()` from the item kind. wire_weaver_derive doc text adapted.
- wip commit 6b3f1026: `wire_weaver_core/src/codegen/server/observe.rs` `batch_items()` rewritten: per trait `<Base>Samples` (+ `<Base>SamplesStore`, `SeriesStore` impl), `<Base>Row { t, values }` (+ `<Base>RowStore`, `SeriesStore`, `LevelRow` impl); `ObserveBatches` has per LEVEL `<field>: Batcher<Row>`, `<field>_gate`, `<field>_cursor` (`Level::batches()` = field ident); samplers take one gate per level and push `Row { t, values }`. Reset uses level gates.

## Next (in order)

1. Finish server observe codegen (observe.rs), see the plan; concretely:
   - `Prop.batch` and `batched_props()` are now unused: remove them.
   - observe_request (`sync_gates`): one level gate from `level_samples()` (`c.<field>_gate.set(..)`), then `state.<outbox>.retain_columns(|k| state.<field>.samples(k as usize).is_some())`. DESC gets `time_slot: Some(item_count)` for batched levels, `None` else (two DESC literals: observe_request and poll_properties).
   - poll encode: `columns` mask from `samples(k)` of batched slots (column c = c-th batched prop in slot order; `SLOTS: [u16; C]` = their table slots k); `budget = batch_budget(&DESC, overhead, buf.len(), OBSERVE_OUTBOX, columns.count_ones())`; `poll_rows` + `push_rows_from`; missed: `polled.lost` and `pushed.lost` are ROWS -> multiply by observed column count; `pushed.dropped` is samples already; on Err discard_sealed. When `level_samples()` is None: flush + discard_sealed.
   - plan_with: due when `outbox.oldest()` has `column(k)`.
   - edf loop: a Samples slot offers the whole oldest batch once via `sel.offer_batch(&ks, time slot_len + Σ batch_slot_len(col))` (ks from `batch.columns()`), `continue` for the other Samples slots; batched props not in Samples mode measure `BatchedOne` as before.
   - write: Samples slot -> `BatchedRaw(oldest.column(k).bytes)`; after all props `if sel.has_time() { wr.write(item_count, &SeriesRaw(oldest.time)) }` (last index, increasing order).
   - next: sent && has_time -> `remove_oldest()`, more = true; a batch whose columns were dropped by the selection (never fits) -> remove_oldest + add_missed (add `Selection::is_dropped(k)` in ww_stream).
   - update_struct: batched level gets `#[slot = item_count] pub sample_times: Option<Series<'i, Second>>` (owned: `SeriesOwned<Second>`; server path `ww_stream::observe::Second`, client `wire_weaver_client::observe::Second`), lifetime when no_alloc; compile_error if a property is named `sample_times`. `batched_slot_ty` is shared with the client codegen.
   - scaffold.rs: Scratch size = biggest single column type (and `Second`) x N, no more `Timed<T>`.
   - Debug generated code: uncomment `debug_to_file` in tests/observe_batched/src/lib.rs, `rustfmt` the file.
2. Typed client (wire_weaver_core/src/codegen/client/observe.rs + wire_weaver_client/src/client/observe.rs): `LevelUpdate::times_mut` visits `sample_times` (unwrap once per level); `Extract::Samples` gets the times: e.g. `fn(U) -> Option<(BatchedOwned<T>, Option<SeriesOwned<Second>>)>`, decode -> `into_samples(raw.t, times)`, error = warn + skip; latest keeps newest sample.
3. tests/observe_batched (+ _api): adapt (Update helper decodes `Batched::Samples` + `sample_times`), add: shared times across properties, level-wide every/max_age grant, a property turned off mid-batch (column dropped, batch still sent for the other), a level batch split over two updates, mismatch rejected on host.
4. Dynamic client (`dyn_observe.rs`: slot `item_count` = `Series<Second>` from `sample_time_ty`, columns `Series<ty>`), `ww observe --samples`, Python `recv_batch()` (t from the clock); compat.rs, api_tree.rs (cli), wire_weaver_py tree.rs use the new fields.
5. Snapshots: `just save-snapshots` (ww_self/ww_client_server changed; `--force` is fine, 0.3.0/0.2.0 never published). Versions: ww_self 0.3.0, ww_client_server 0.2.0 already above crates.io: no bump; workspace crates check `cargo info --registry crates-io`.
6. CHANGELOGs (Unreleased): shrink_wrap (RawPlain done), ww_stream, ww_client_server, ww_self, wire_weaver_core, wire_weaver_client, wire_weaver_cli, wire_weaver_py. FEATURES: claim an id (`tpm work claim API`), item for I-2 shared clock, `just features-page`, entries.md line. docs/api/observing.md "Every sample" section (wire, time slot, level-wide settings, RAM/CPU numbers), docs/evolution/rules.md if it mentions Batched::Series. batching-docs session (gpd-omarchy) writes docs/api/batching.md against the old shape and rebases after this lands; tell it when ready.
7. Gates: just test, just lint, just check-mcu, just check-examples-mcu, just build-docs (allowed). Then `git rm HANDOFF.md`, `tpm work ready`.

## Gotchas

- QEMU runs: on gpd-omarchy only, scratch `/tmp/shared-clock-bench` there, removed after (omarchy-m1 has no qemu-system-arm, is aarch64). Never unbounded traces.
- `probe-rs run` of the H725 bins idles in wfi after "done": stop it on "done", don't pipe into `head`.
- `#[derive_shrink_wrap(series)]` inside a `macro_rules!` breaks (hygiene), filed P2401#18oct-1150.
- Batcher `set_budget_bound` resets the measured `last` when the bound (column set) changes.
- `Outbox::new` needs BYTES > 12 now.
