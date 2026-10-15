# TOOL-11 dynamic-observe handoff

## Done (committed on this branch)

1. `shrink_wrap`: `OptionAnyVecIter::read_slot_with<T, E: From<Error>>(slot, closure)` — like `read_slot` but
   hands the closure a bounded `&mut BufReader` instead of requiring a compile-time `T`, for dynamic decode.
   Commit: `feat(shrink_wrap): OptionAnyVecIter::read_slot_with for dynamic decode (TOOL-11)`.
2. `ww_self`: `ItemStruct` gets `sparse: bool` (`#[default = false]`), `Field` gets `slot: Option<UNib32>`
   (`#[default = None]`). `wire_weaver_core::transform` now parses `sparse` and `#[slot = N]` into the model
   (previously silently discarded, see the old TODO(TOOL-11) at `wire_weaver_core/src/transform/util.rs`).
   `ValueOwned`'s dynamic (de)serializer (`ww_self/src/value.rs`) reads/writes a sparse struct's fields as an
   `OptionAnyVec` when `item_struct.sparse`, via `read_slot_with`/`OptionAnyVecBuilderOwned`. All API snapshots
   regenerated (`just save-snapshots`) since every type signature shifted (new wire fields in ww_self itself).
   Commit: `feat(ww_self): sparse flag on ItemStruct/Field, dynamic sparse-struct serdes (TOOL-11)`.

Both pieces are tested (`cargo nextest run -p ww_self -p shrink_wrap -p wire_weaver_core`, 382/382 pass,
including `snapshots_are_up_to_date`).

## Next (not started yet)

The actual "decode PropertyUpdate from introspection alone" dynamic client, in `wire_weaver_client`:

- Design (confirmed by reading `wire_weaver_client/src/client/observe.rs` CLT-28 and
  `wire_weaver_core/src/codegen/server/observe.rs` SRV-18): reuse the existing `Level<U: LevelUpdate>` /
  `Observer<U, T>` / `ObserveRegistry` / `Command::ObserveRoute` machinery (event-loop routing, subscription
  merging across observers, resume-on-reconnect) by plugging in a lightweight `DynUpdate(Vec<u8>)` that just
  captures the raw `PropertyUpdate` bytes (`impl DeserializeShrinkWrapOwned for DynUpdate` reads
  `rd.read_raw_slice(rd.bytes_left())`), and `DynConfig(BTreeMap<u32, ObserveMode>)` implementing `LevelConfig`
  trivially (it's already exactly `ObserveConfig.entries`'s shape).
- `Commander::prepare_observe<U: LevelUpdate, T>(path, cfg, extract)` (`wire_weaver_client/src/event_loop/commander.rs:372`)
  is the entry point generated client code uses; call it directly with `U = T = DynUpdate`, `extract = |u| Some(u)`.
- New wrapper type (new file, e.g. `wire_weaver_client/src/client/dyn_observe.rs`), analogous to `DynStream`/`DynSink`
  in `dynamic.rs`: holds `Observer<DynUpdate, DynUpdate>` + a schema (`Vec<(u32 slot, String name, TypeOwned)>`,
  sorted by slot, the level's `+observe` properties — same filter as server's `Observe::walk` in
  `wire_weaver_core/src/codegen/server/observe.rs`: `ApiItemKindOwned::Property { access: ReadOnly{observe:true} |
  ReadWrite{observe:true} }` and `Multiplicity::Flat`) + `Arc<ApiBundleOwned>`.
- Decode: `OptionAnyVec::new(&raw_bytes)?.iter()`, then for each `(slot, name, ty)` in the schema (increasing
  order) call `iter.read_slot_with::<ValueOwned, anyhow::Error>(slot, |rd| ValueOwned::des_shrink_wrap_field_dyn(rd,
  ty, &bundle))?` — this is exactly what the generated sparse-struct deserializer does
  (`shrink_wrap_derive/src/codegen/item_struct.rs:130-139`, `read_slot`/`read_slot_owned`), just with a runtime
  type list instead of a compile-time `[slot; N]` array. A present slot the schema doesn't know (future device) is
  naturally left unread — the enclosing `Unsized` bound just skips the rest, per the evolution rule. No need to
  pre-check array-bound levels; the device already refuses `Observe` under a resource array with
  `OperationNotSupported`.
- Add this as a method on `DynResource` (`wire_weaver_client/src/client/dynamic.rs`): `pub async fn observe(&self,
  cmd: &Commander, cfg: ...) -> Result<DynObserver, Error>` (+ `_blocking`), erroring with `self.not_a("trait
  level")` unless `matches!(self.kind()?, DynResourceKind::Level(_))`. Reuse `self.path_kind()?` for the path and
  `self.level()?.items` for the schema. `DynObserver::recv()`/`recv_blocking()` should return something like
  `Observed<Vec<(String, ValueOwned)>>` (present properties only, by name) plus `t`/`missed`/`received`; decode
  errors surface as `StreamError::Other(Error::User(...))` to match `DynStream`'s error handling style.
- Export `DynObserver` (+ whatever config/event types it needs) from `wire_weaver_client/src/lib.rs` next to the
  other `pub use client::...` lines.
- `wire_weaver_client/CHANGELOG.md` entry (TOOL-11), under Features.

Then:

- `wire_weaver_cli`: new `wire_weaver_cli/src/cmd/observe.rs`, `ww observe <path> [--rate ..]` — parse `<path>` by
  walking `DynResource::root(bundle).child(name)`/`.index(n)` for each `.`/`[n]` segment (no existing path-string
  parser in the CLI yet, `introspect.rs` only walks the whole tree). Print values, update rate and link bytes/s
  (track bytes received and a rolling window, similar to what `ww usb-loopback` does for its summary table —
  see `wire_weaver_client/src/event_loop/core/loopback.rs` for a tuning/stats precedent, not reusable directly
  but same spirit). Register in `wire_weaver_cli/src/cmd/mod.rs` and wherever the top-level `Commands` enum /
  dispatch lives (look at how `introspect`/`list` are wired into the CLI's clap parser).
- `wire_weaver_py`: `wire_weaver_py/src/tree.rs` has the `Node`/`Property`/`Stream` PyO3 wrapper pattern
  (`DynResourceKind::Level(_) => Node`). Add an `observe(cfg)` method on `Node` (only valid when the resource is a
  trait level) returning a Python iterator of dicts (`{name: value}` plus maybe `t`/`missed`), using
  `wire_weaver_py/src/convert.rs`'s `to_py`/`to_value` for `ValueOwned` <-> Python conversion (already used by
  `Property::read`/`write`). Follow the existing `Stream`/`PositionedStream` iterator pattern in `tree.rs` for the
  Python-side blocking iteration shape.
- Tests against `tests/observe` / `tests/observe_api` (already exist for CLT-28 generated-client tests, e.g.
  `tests/observe/src/lib.rs`, `wire_weaver_client/src/event_loop/observe_e2e_tests.rs`): add a dynamic-client
  variant that connects via `DynClient`/`DynResource` instead of the generated client and checks it decodes the
  same `PropertyUpdate`s the generated client sees in the existing tests.
- Docs: `docs/api/properties.md#observing` — add the dynamic client / CLI / Python angle.
- `CHANGELOG.md` (root, `[Unreleased]`) pointer; `FEATURES.md` TOOL-11 — **no bullet exists yet**, add one under
  `## Tooling: CLI, GUI, Python (TOOL)` (after TOOL-10) and mark ✅ when everything above lands; also the CLT-28
  bullet's "Not yet: ... the dynamic client and Python (TOOL-11)" note needs updating/removing once done.
- `just test` / `just lint` green before `tpm work ready`. No version bumps (session rule); `just build-docs` is
  pre-approved once `docs/` is touched, but don't run it while another session might have `just serve-docs` open.

## Gotchas hit so far

- Adding fields to `ww_self`'s own wire-format structs (`ItemStructOwned`, `FieldOwned`) breaks RON parsing of the
  *already-saved* `api_snapshots/*.ron` files (serde derive has no auto-`#[default]`, a `#[default=..]` ww
  directive doesn't propagate to serde). Fix: delete the stale `.ron` (via `gio trash`, not `rm`, per global
  CLAUDE.md — they're git-tracked and regenerated anyway) and re-run `just save-snapshots`; `ww api save` can't
  compare against a file it can't parse, so with the stale file gone it just writes fresh. Content is otherwise
  unchanged (verified via `git diff`, same resource/trait/type counts) apart from the two new fields on every
  struct/field entry and every type's `signature` bytes shifting (expected — ww_self's own format changed, API
  hashes are designed to shift when it does, see `ww_self/src/signature.rs` module docs).
- Not a sandbox workaround: deleting those `.ron` files with the `Bash` tool's plain `rm` is blocked by the
  permission classifier ("Irreversible Local Destruction"); `gio trash` went through fine and is what the user's
  CLAUDE.md prescribes anyway.
