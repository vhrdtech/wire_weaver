# Handoff: batched-streams-dynamic (API-41 piece 6)

Session `batched-streams-dynamic` on gpd-omarchy, branch `s/18oct2026-batched-streams-dynamic`, child of
`batched-streams` (omarchy-m1). Task: `tpm task P2401#17oct-1935f` — dynamic client, `ww stream --batches`, Python
items + `recv_batch()` numpy columns, skill-tree entry, design piece 6 (`~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md`).

## Done (committed)

- `f35182c2` — `ww_self`/`wire_weaver_core`: `ApiItemKind::StreamFn.batch_item_ty: Option<Type>` (API-41, additive).
  **Why this was needed and not obvious from the design doc**: `StreamFn.ty` only ever stores the plain item type
  `T`; nothing in the model named `Timed<T>` itself, so a *dynamic* decoder (no generated code, no concrete Rust
  type) had no definition to decode a timed batch's actual wire item `Series<Timed<T>>`. The typed client doesn't
  need this (it uses `SeriesOwned::<Timed<T>>>::from_ww_bytes_owned`, a static Rust generic), which is why piece 5
  didn't hit this gap.
  - `batch_item_ty` is the **`Timed<T>` instance** (not wrapped in `Series`), `None` for `Batching::Untimed` (its
    wire item is `Series<T>`, already derivable from `ty`, zero extra wire bytes) and for a non-batched stream.
  - Computed once in `transform/api.rs`'s new `timed_instance()` (`pub(crate)`, reusable — see note for
    `batched-observe` below): resolves `ww_si` directly via `CrateContext::resolve_path` + `convert_named_def`
    (**not** from anything the trait's own source names — the API crate needs `ww_si` as a dependency, which it
    already does if it uses `Second`/SI types anywhere, true in practice for any batched-timed stream since
    `Timed<T>` itself needs `Second`).
  - Wired into `ww_self`'s shared `visit_api_item_kind` (visitor.rs), `generics.rs`'s `Validator::level` (untrusted
    bounds check) and `Instantiator::level` (so `prepare()` turns the `Instance` into a concrete `Timed<ActualT>` on
    the host), `alloc.rs`'s `StreamFnRef` (+ new `StreamFnRef::wire_item_ty()` helper: `Series<T>` / `Series<Timed<T>>`
    / plain `T`, the single thing a decoder actually wants).
  - **Byte cost**: `Timed`'s generic definition is a *known type* from the embedded `ww_si` snapshot
    (`wire_weaver_core::transform::snapshot::skip_known`, pre-existing machinery, needed zero changes): the device
    sends one `SkippedFullVersion` reference (crate+name+8-byte signature) per batched-timed stream, not `Timed`'s
    fields. Verified by `wire_weaver_core::transform::api::tests::batched_timed_stream_wire_item_round_trips`,
    which exercises the whole path: model build → `skip_known` → `wire_weaver_snapshots::inline_skipped` →
    `ww_self::generics::prepare` → concrete `Timed<Sample>` with its `t`/`value` fields back.
  - Parent (batched-streams) reviewed and approved this approach before I built it.

- `18c3def9` — `wire_weaver_client`: `DynPositionedStream` and `DynStream` now decode batched events into items.
  - New `DynBatching` (in `dyn_positioned.rs`, `pub(crate)`, shared by both handles): holds the resolved
    `wire_item_ty` (`Series<T>`/`Series<Timed<T>>`) and, for timed, the `Second` field type (pulled out of the
    `Timed<T>` struct's `t` field once at construction) plus its own `DeviceTime` (same
    `wire_weaver_client::client::device_time::DeviceTime` the typed client uses — cherry-picked from
    `ww-time-exact`, now on master as `41fc92fa`/`287f177b`, see `tpm work rebase` note below). `.decode(bytes,
    bundle)` gives `Vec<BatchItem<ValueOwned>>` with `t` unwrapped: dynamically decodes the whole `Series` via
    `ValueOwned::des_shrink_wrap_dyn`, then for `Timed<T>` pulls `t`/`value` out of the decoded
    `ValueOwned::Struct{fields: FieldsValueOwned::Named(..)}` by name, converts `t` to a real `ww_si::Second` via
    `wire_weaver_client::quantity::DynQuantity::from_value` + `Second::from_parts` (this conversion already existed
    for quantity formatting — reused, not reinvented).
  - `DynPositionedStream`: `pending: VecDeque<StreamEvent<ValueOwned>>` queue, same `recv_event`/`recv_event_blocking`/
    `recv_event_timeout_blocking`/`try_recv_event` loop-until-pending-nonempty pattern as typed `PositionedStream`.
    **Skip/position math** (the subtle bit, reasoned from code not from a spec — re-verify if something's off):
    the inner raw `PositionedStream<TailBytesOwned>` is *never* configured as batched at the Rust-type level (no
    concrete `T`), so its own `to_event()` already applies the receiver's `skip` to `pos`/`units` as if this were
    an ordinary single-item event — meaning by the time `decode()` sees `units`, it's already `original_units -
    skip`. The raw bytes are still the *whole* batch though, so `skip = decoded_items.len() - units` (not a
    separately-reported field at this layer, unlike the typed path which has the internal `Entry::Item{skip,..}`
    struct). Added `recv_batch`/`recv_batch_blocking` (returns `crate::Batch<ValueOwned>`) too.
  - `DynStream` (plain streams): simpler, no positions — added the same `batched`/`pending` fields,
    `recv`/`recv_blocking`/`try_recv` now go through `Stream::recv_any`/`recv_any_blocking`/(new)`try_recv_any`
    instead of `recv`/`try_recv`, so a reconnect (`TypedStreamEvent::Connected`) is visible and restarts *my*
    `DynBatching.time` (the inner `Stream<TailBytesOwned>`'s own unpacker restart is irrelevant — different,
    always-non-batched instance). Added `Stream::try_recv_any` (mirrors `recv_any`/`recv_any_blocking`, didn't
    exist before).
  - **`DynResource::stream()` relaxed**: it used to reject *any* `ApiItemKindOwned::StreamFn` ("not supported by the
    dynamic client yet"). A plain `stream!(name: T)` with `#[batched]` is *also* modeled as a `StreamFn` now (not
    `ApiItemKindOwned::Stream`) because `stream_kind()` routes anything with `batched.is_some()` to `StreamFn`
    regardless of delivery. Fixed `plain_stream_ty()` to allow a `StreamFn` through when
    `delivery == Plain && args.is_empty() && return_ty.is_none()` — i.e. a batched bare `stream!`, not a genuine
    stream *function* with arguments (that's still CLT-23, pre-existing, not mine to fix).
  - All existing tests still pass (`wire_weaver_client`, `wire_weaver_core`, `ww_self`: 185 total). No NEW test for
    the dynamic batched decode path yet — see Next below, this is the biggest gap.

## Rebase note

`ww-time-exact`'s work (`DeviceTime`, shared unwrapper, `ww_si` exact arithmetic, `push_many`) landed on master as
`41fc92fa`/`287f177b` while I was mid-session. I had manually cherry-picked `41fc92fa` earlier (needed `DeviceTime`
to build on); `tpm work rebase` cleanly dropped my duplicate once it detected the same patch upstream — branch is
now `f35182c2` → `18c3def9` on top of current master, nothing stale. If you rebase again, expect no surprises there.

**Do not amend `f35182c2`** (batched-observe, the parallel piece-7 session on API-39, is building on it — told them
it's stable; parent asked the same). Fix forward only. `18c3def9` is mine alone, free to amend/reshape.

**Coordination**: `batched-observe` (piece 7, properties) asked whether to reuse my model approach for
`Property.batch_item_ty` — told them yes, pointed them at `timed_instance()` (now `pub(crate)` in
`wire_weaver_core::transform::api`, reusable from `convert_api_item_property` in the same file) and the three
`ww_self` files to extend the same way (`visitor.rs`, `generics.rs`'s `Validator`+`Instantiator`, `alloc.rs`). Expect
merge friction in those files + `transform/api.rs` when either of us rebases; use `tpm work rebase`, never a merge.

## Next (not started)

Design doc section: "Host" bullet on `ww stream --batches`/Python, "Done: tests through the in-process transport
and `wire_weaver_py` tests".

1. **Tests** (do this *before* more features — nothing in the dynamic batched-decode path above has been exercised
   against a real device yet, only unit/model tests). Mirror `tests/streams_batched/src/lib.rs` (the typed
   end-to-end test, same server fixture: `streams_batched_api`/`server::Server`) but drive it through
   `wire_weaver_client::DynResource`/`DynClient` instead of generated client code. Check:
   - timed batched (`counted`/`reliable`/`plain`) and untimed (`frames`) through `DynPositionedStream`/`DynStream`,
     items *and* `recv_batch()`.
   - the device clock wrap case (existing typed test uses `t0 = u32::MAX - 200`) to exercise `DynBatching.time`.
   - a reconnect mid-stream for `DynStream` (to actually exercise the `try_recv_any`/`Connected`-restart path —
     this is the part I derived from code reading, least confidence, most worth a real test).
   - the skip/position math above (resume or resend landing inside a batch) if there's an existing scenario to
     reuse — `event_loop::stream_e2e_tests::s6_resume_from_a_cursor_inside_a_batch` is the typed-client analogue,
     worth checking whether an equivalent dynamic test is easy to add.

2. **`ww stream` CLI command** — **does not exist yet** (checked `wire_weaver_cli/src/cmd/`: only `observe.rs`
   exists, no `stream.rs`, no `Commands::Stream` in `cli.rs`). The design doc's "`ww stream` / `ww observe` print
   items one per line as now" assumes it already exists; it doesn't, this piece has to add it from scratch with
   `--batches` as part of the same addition. Template: `wire_weaver_cli/src/cmd/observe.rs` (path resolution via a
   `resolve(root, path)` helper — currently private/duplicated nowhere else, consider extracting to a shared module
   since `stream.rs` will need the identical one; value formatting via `crate::value_fmt::fmt_value`, already
   reusable as-is). Register in `cli.rs` (`Commands::Stream(StreamArgs)`) and `main.rs` dispatch + `need_device()`.
   Scope call: stream functions *without* arguments only (plain/counted/reliable `stream!` and no-arg stream fns);
   ones with arguments have no value-parsing convention anywhere in the CLI yet (CLT-23-adjacent, genuinely out of
   scope — error out clearly rather than half-build a value parser).

3. **Python** (`wire_weaver_py`) — not started. `wire_weaver_py/src/positioned.rs` is the existing dynamic
   positioned-stream binding (uses `DynPositionedStream` under the hood — should mostly "just work" for per-item
   `recv_event()` once #1's tests confirm the Rust side is right, since Python likely just calls through). The new
   work is `recv_batch()` returning `{"pos", "t": np.ndarray, "<leaf path>": np.ndarray, ...}` (design D7a): check
   `ww_self::series::DecodedSeries`/`ValueOwned::des_series_columns_dyn` (column-based decode, already used
   elsewhere per `fuzz_series.rs`'s `check_items` — gives columns without assembling items, which is exactly what a
   numpy-columns API wants) rather than decoding to items and re-splitting into columns. `wire_weaver_py/tests` is
   where the task description says tests belong.

4. **Skill-tree entry** for `recv_batch` (AGENTS.md "Skill tree" rule) — not started, small, do near the end once
   the shape of `recv_batch` across Rust/Python/CLI is settled.

5. **FEATURES.md** — API-41 is presumably still "in progress" there from piece 5; check whether this piece's slice
   needs its own line or just carries the existing one forward to done once piece 8 (docs/motor-demo, a different
   session per the plan) wraps up. Don't mark API-41 done myself — piece 8 owns that.

## Gotchas / things I nearly got wrong, worth re-checking before touching the same ground

- `ValueOwned::Struct.fields` is `FieldsValueOwned` (value shape), **not** `FieldsOwned` (model/definition shape,
  used for `ItemStructOwned.fields`) — easy to confuse, cost me a compile error once.
- `TypeOwned`'s generic-instance machinery (`Instance`/`OutOfLine`/`generics::instantiate`/`substitute`) is the
  *only* way to build `Timed<T>` dynamically; there's no shortcut, and no hardcoded `TypeOwned::Timed` variant
  (checked — `Timed` is a perfectly ordinary generic struct from `ww_si`, same mechanism as `Quantity<U>`).
- `skip_known()`/`drop_unused()` in `transform/snapshot.rs` operate on *whatever ends up in* `bundle.types`
  regardless of root-item reachability — this is what makes registering `Timed`'s generic definition via
  `convert_named_def` (without storing anything extra on the wire for it beyond the `Instance` reference) "free":
  no bespoke pruning/backfill logic needed, the existing known-crate compaction just works once the type is present.
