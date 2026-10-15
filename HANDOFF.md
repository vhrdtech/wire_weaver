# TOOL-11 dynamic-observe handoff

Context checkpoint: conversation is past its soft token limit, handing off via `tpm work handoff` after this note.
Everything below is committed on this branch (`s/15oct2026-dynamic-observe`).

## Done

1. `shrink_wrap`: `OptionAnyVecIter::read_slot_with<T, E: From<Error>>(slot, closure)` — like `read_slot` but
   hands the closure a bounded `&mut BufReader` instead of requiring a compile-time `T`.
2. `ww_self`: `ItemStruct` gets `sparse: bool` (`#[default = false]`), `Field` gets `slot: Option<UNib32>`
   (`#[default = None]`). `wire_weaver_core::transform` now parses `sparse` and `#[slot = N]` into the model.
   `ValueOwned`'s dynamic (de)serializer reads/writes a sparse struct's fields as an `OptionAnyVec` when
   `item_struct.sparse`. All API snapshots regenerated (`just save-snapshots`, every type signature shifted since
   ww_self's own wire format changed — expected, see `ww_self/src/signature.rs` module docs).
3. `wire_weaver_client`: `wire_weaver_client/src/client/dyn_observe.rs` (new) — `DynUpdate`/`DynConfig` (a
   schema-less pair plugging into the existing `Level<U>`/`Observer<U,T>`/`Commander::prepare_observe` machinery
   from CLT-28), `LevelSchema` (the level's `+observe` properties by resource index, from introspection),
   `DynObserver` (`recv`/`recv_blocking`/`try_recv`/`granted`/`configure`/`configure_blocking`/`dropped`),
   `DynObserved` (own struct: `values`, `t`, `missed`, `received`, `data_len` — not a bare `Observed<T>` alias,
   because `ww observe`'s bytes/s needs the raw update size `Observed<T>` doesn't carry).
   `DynResource::observe()`/`observe_blocking()`/`observable_properties()` in `dynamic.rs`.
4. `tests/observe`: two new tests (`dynamic_client_observable_properties`, `dynamic_client_observes_a_level`)
   alongside the existing CLT-28 generated-client ones, same Motor level, asserting the same decoded values.
5. `wire_weaver_cli`: `ww observe <path> [--rate MS]` (`wire_weaver_cli/src/cmd/observe.rs`), wired into
   `cli.rs`/`main.rs`. `<path>` is dotted/indexed (`motor`, `bank[0]`), empty for the API root. Prints each
   update's values (via the existing `value_fmt::fmt_value`), a measured update rate and link bytes/s
   (`human_repr`), until Ctrl+C.
6. Changelogs done: `shrink_wrap/CHANGELOG.md`, `ww_self/CHANGELOG.md`, `wire_weaver_core/CHANGELOG.md`,
   `wire_weaver_client/CHANGELOG.md`, `wire_weaver_cli/CHANGELOG.md`, root `CHANGELOG.md` `[Unreleased]` pointer.
   `FEATURES.md`: new **TOOL-11** bullet (🚧, under `## Tooling: CLI, GUI, Python`, after TOOL-10) says what's done
   and names the one thing left ("Not yet: `wire_weaver_py`'s `observe(level, cfg)`"); the DER-13 bullet and the
   CLT-28 bullet's stale "not yet" notes were both updated.

Verified per-crate: `cargo nextest run -p shrink_wrap -p ww_self -p wire_weaver_core -p wire_weaver_client -p
observe` (all green, including `snapshots_are_up_to_date` and the two new dynamic tests), `cargo clippy` and
`cargo fmt --check` clean for every touched crate. `just check` (whole-workspace, slower: `mcu/`,
`examples_mcu/`) was kicked off in the background right before this note and never confirmed — **run it (and
`just test`, `just lint`) before `tpm work ready`**, nothing touched here should affect `mcu/`/`examples_mcu/`
but it hasn't actually been checked.

## Next

1. **Confirm `just check` / `just test` / `just lint` are green** (see note above) before anything else.
2. **`wire_weaver_py`** — the one remaining TOOL-11 deliverable per the task: `observe(level, cfg)` iterator of
   dicts.
   - `wire_weaver_py/src/tree.rs` has the PyO3 wrapper pattern: `wrap()` dispatches on `DynResourceKind` to
     `Node`/`Array`/`Method`/`Property`/`Stream`/`PositionedStream`/`Sink` (all hold `res: DynResource, conn:
     Option<Arc<Conn>>`). Add an `observe(cfg)` (and maybe `observable_properties()`) method on `Node` — only
     valid when `res.kind()` is `DynResourceKind::Level`, error otherwise (match the `WireWeaverError`/`client_err`
     style already used, e.g. in `wrap()`/`conn()`).
   - Need a Python iterator object wrapping `wire_weaver_client::DynObserver`, analogous to how `Stream`/
     `PositionedStream` wrap their Rust stream type in `tree.rs` (`Mutex<Option<...>>`, opened lazily on first
     `__next__`/an explicit call — check exactly how `Stream`'s `#[pymethods]` `__iter__`/`__next__` are shaped,
     they're the direct precedent). Yields a `dict` per update: property name → Python value (via
     `convert::to_py`, already used by `Property::read`), plus probably `t`/`missed` as extra dict keys or a
     second return value — follow whatever convention `Stream`'s iterator already uses for its item shape (check
     if it's a bare value or a dict with metadata) and stay consistent rather than inventing a new shape.
   - `cfg` (Python side) is presumably a dict `{property_name: rate_ms}` or similar — needs converting to the
     `BTreeMap<u32, ObserveMode>` `DynResource::observe()` takes: look up each name's resource index via
     `observable_properties()`, and decide a Python-side mode encoding (a plain milliseconds number = periodic,
     `None`/omitted = on-change — mirror the CLI's `--rate` semantics in `wire_weaver_cli/src/cmd/observe.rs` for
     consistency, don't invent a different one).
   - `wire_weaver_py/CHANGELOG.md` entry, `wire_weaver_py/tests/test_offline.py` if an offline-testable path exists
     (unlikely, since observing is live-device-only — check what the existing `Stream`/`Property` Python tests
     cover, there's probably no offline stream test either, meaning no analogous test is expected here).
   - Once done: update `FEATURES.md` TOOL-11 to ✅, remove the "Not yet" clause, move it up among the done items of
     `## Tooling: CLI, GUI, Python (TOOL)` per the file's own convention (see `## How to use this file` at its
     top).
3. **Docs**: `docs/api/properties.md#observing` currently only documents CLT-28 (generated client). Add the
   dynamic-client / CLI / Python angle there (a short subsection, following the existing structure — read the
   whole `#observing` section first, it's substantial). After touching `docs/`, `just build-docs` is pre-approved
   (Roman, 16 Oct) **but not if `just serve-docs` might be running in another session** — check before running it.
4. Re-run `just test`/`just lint` after the docs + Python additions, then `tpm work ready --note "..."`.

## Design notes worth keeping in mind

- The dynamic decode deliberately mirrors what the generated sparse `<Trait>Update` struct's deserializer does
  (`shrink_wrap_derive/src/codegen/item_struct.rs:130-139`): iterate the schema's `(slot, type)` pairs in
  increasing order, `read_slot_with` each one. A present slot past the schema's last known one is simply never
  read — the enclosing `Unsized` bound skips it — so there's no "unknown slot" handling needed; this only breaks
  (`OptionAnyVecUnknownSlot`) for a genuine gap *before* a known slot, which can't happen against a live device's
  own introspection (the schema always matches exactly what that device currently advertises).
- `DynConfig`/`DynUpdate` are intentionally dumb pass-through types so the *existing* `Level<U>`/`Observer<U,T>`
  machinery (subscription merging across observers, resume-on-reconnect, one device subscription per level) is
  reused unchanged — don't duplicate that logic for the dynamic path.

## Gotchas hit

- Adding fields to `ww_self`'s own wire-format structs (`ItemStructOwned`, `FieldOwned`) breaks RON parsing of the
  *already-saved* `api_snapshots/*.ron` files (serde derive has no auto-`#[default]`; a ww `#[default=..]`
  directive doesn't propagate to serde). Fix: delete the stale `.ron` (via `gio trash`, not `rm` — they're
  git-tracked and regenerated anyway, and the global CLAUDE.md says trash outside `/tmp`) and re-run
  `just save-snapshots`; `ww api save` can't compare against a file it can't parse, so with the stale file gone
  it just writes fresh. Content is otherwise unchanged (verified via `git diff`, same resource/trait/type counts).
- Plain `rm` on those `.ron` files was blocked by the sandbox's permission classifier ("Irreversible Local
  Destruction"); `gio trash` went through fine and is what the user's CLAUDE.md prescribes anyway — not a
  workaround, the correct tool for the job.
