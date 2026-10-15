# CLT-15 multi-write-call client — handoff

## Task
Generalize the client-side multi-request API (`MultiRead`/`MultiCall`/`MultiWrite`): today only `MultiRead`
exists, as a hand-written 2-tuple-at-root-only trait impl. Need: N-tuple (not just 2), any depth (not just root),
`MultiCall`/`MultiWrite` siblings, homogeneous array requests (Range/List/Mask32 across an array's indices,
`MultiArgs::Same`/`Different`), and a dynamic-client (`DynResource`) equivalent. Tests in `tests/properties` and
`tests/methods`. CHANGELOG + FEATURES `CLT-15` ✅. No version bump anywhere (`wire_weaver` lands with
`--no-bump`; the shared workspace version and `ww_client_server`/`ww_self`/`shrink_wrap` are already bumped above
their last crates.io release from earlier work this cycle, so nothing more to bump for additive changes here —
checked each with `cargo info --registry crates-io <crate>` before touching, don't re-check unless you suspect it
changed).

Device side already landed on master (e6fb43c3, 45fd6f1b) before this session started: `MultiIndexIter`
(Range/List/Mask32), `RequestKind::MultiCall`/`MultiWrite` dispatch, deferred methods refused with
`OperationNotSupported` inside a multi request.

## Status: implementation + tests DONE, only CHANGELOG/FEATURES/docs polish + `just test`/`just lint` left

Commits on this branch so far (all build clean, clippy clean on touched crates, no_std workspace `cargo check
--workspace` passes):
1. `f439d7bc` feat(client): generalize multi-request client API, any depth/arity (CLT-15) — the core design (see
   below), tuple macro (arities 2..=8) for `MultiRead`/`MultiCall`/`MultiWrite`, array free functions
   (`read_array`/`call_array`/`write_array` + `_blocking`), `ww_client_server::MultiIndexOwned::iter_u32`/`len`/
   `is_empty`. CHANGELOGs for root, `wire_weaver_client`, `ww_client_server` already done.
2. `911a9b48` feat(client): DynResource multi_read/multi_write/multi_call for arrays (CLT-15) — dynamic-client
   equivalent, `DynMultiArgs`/`DynMultiValue`.
3. `f457e9a6` test(properties,methods): array fixtures — `properties_api::Channel` (`ch[]`, property `level`),
   `methods_api::Unit` (`unit[]`, method `double`), `multi_req = true` server handlers for both (methods' only
   server didn't have `multi_req` before, now does — harmless for its other tests).
4. `e2316959` fix(client): decode dynamic multi-request items without overrunning (CLT-15) — **the one real bug**
   found while writing the dynamic-client test: `DynResource::multi_read`/`multi_call` decoded each `EitherAnyVec`
   item as `TailBytesOwned`, which reads to the end of the *whole* buffer, not just this item — broke on every
   item except the last whenever the dynamic type was `Sized` (e.g. an `f32` property has no size in the
   trailer to bound it; only happened to work when the type was `Unsized`). Fixed with two small additive API
   surfaces: `shrink_wrap::EitherAnyVecIter::next_with()` (hands the caller a properly-bounded `BufReader` for
   whichever side was flagged) and `ww_self::ValueOwned::des_shrink_wrap_field_dyn()` (reads one dynamically-typed
   value the way a struct field is read, bounded to its own size first when unsized) — this is the mechanism
   `ww_self::value::des_field` (private) already used internally for struct fields, now exposed for this use.
   If you're debugging something that looks like "field N decoded, field N+1 is garbage" anywhere dynamic-typed
   values are packed with others, this is the pattern to reach for.
5. `07387646` test(properties,methods): N>2 tuple and array multi-request tests — 3-tuple `MultiRead`/`MultiWrite`
   in `properties` (x/y/custom; `write_mode` skipped on purpose, its `CallError<ModeError>` is outside the tuple
   API's scope — see "Known limitations" below), 3-tuple `MultiCall` in `methods`
   (plain_return/request_seq/request_medium), `array_multi_requests` / `multi_call_array` (list/range/mask32,
   `Same`/`Different`, per-item `BadIndex`) in both.

### Design (implemented, for reference — read the doc comments at the top of `wire_weaver_client/src/client/multi.rs`
and `DynResource::multi_read` in `client/dynamic.rs` for the authoritative version)
Key insight: both "different resources at one level" (the original 2-tuple `MultiRead`) and "same resource across
an array" (e.g. call `set_level` on pins 0..4) reduce to the same shape: N absolute paths of equal length that
agree on a prefix, diverge at exactly one position (becomes `multi_idx`), and optionally agree again on exactly
one more trailing id after that (becomes `in_each_array_id`). `split_common()` in `multi.rs` implements this once,
used by both the tuple macro and (indirectly, via each array index's materialized path) the array functions.
`MultiArgs::Same`/`Different` is chosen automatically by comparing serialized bytes — equal → `Same` (smaller on
the wire), otherwise `Different`. No codegen changes anywhere — everything is generic code in `wire_weaver_client`
reusing the already-generated per-resource `PreparedRead`/`PreparedCall`/`PreparedWrite` builders.

1. **Heterogeneous tuple API**: `MultiRead`/`MultiCall`/`MultiWrite` traits impl'd for tuples via `macro_rules!`
   for arities 2..=8 (`multi.rs`), generic over `PropertyPath`/`CallPath`/`WritePath` (new crate-internal `pub`
   traits — `pub` because their `Output` associated type leaks into the public trait's return type; `#[doc(hidden)]`
   since users never implement them themselves).
2. **Homogeneous array API**: `read_array`/`call_array`/`write_array` (+`_blocking`) take the user's own
   `MultiIndexOwned` selector (kept verbatim on the wire, not rebuilt as a `List`) plus a closure that's just the
   existing generated per-index accessor, e.g. `|i| client.pin(i).read_mode()` — called once per index locally
   (no network traffic) to extract path/bytes for `split_common`, but the *selector itself* is what's sent.
3. **Dynamic client**: `DynResource::multi_read`/`multi_write`/`multi_call` on a `Node::Array` resource, with
   `child: Option<&str>` (array-of-trait vs `None` for a flat array like `property!(led[]: bool)`). Resolves the
   child's id/types by building a throwaway `self.index(0).child(name)` purely for local type lookup (never sent),
   reusing existing `call_args`/`decode_return`/etc. helpers. Needed the `next_with`/`des_shrink_wrap_field_dyn`
   fix above to decode correctly.

`TransportCommander` (in `event_loop/commander.rs`) is now `pub` (was `pub(crate)`) so the new-and-public
`PropertyPath`/`CallPath`/`WritePath` traits can return it from a method without tripping the `private_interfaces`
lint — still unreachable from outside the crate since `event_loop` itself stays `pub(crate)`. Likewise
`Commander::transport()` went from private to `pub(crate)` so `dynamic.rs` can reach it.

### Known limitations (intentional scope cuts, not oversights — mention if asked, don't "fix" without checking first)
- Tuple traits (`MultiRead`/`MultiCall`/`MultiWrite`) only work with `PreparedCall<T, Error>` /
  `PreparedWrite<Error>` — a method/property with a custom `#[errors(..)]` result type (`CallError<E>`) can't go
  in a tuple. This matches the *original* `MultiRead`'s scope (it never handled `CallError` either); extending it
  would need per-impl trait bounds parameterized over `E: OutcomeError` plus a way to decode a `CallError`'s
  `Device(E)` variant from inside an `EitherAnyVec` item (same category of bug class as commit 4 above — the
  `UserBytes` wrapping adds another layer of Sized/Unsized bounding to get right). Not attempted.
- Tuple traits are async-only, no `_blocking` or `_promise` variants (array functions have `_blocking`, no
  `_promise` either). Matches prior `MultiRead`'s shape. A real gap if a GUI/immediate-mode user wants this; flag
  it to the user rather than silently adding without discussing API shape (promise plumbing is nontrivial, see
  `prepared_read.rs`'s `read_promise`).
- `read_array`/`call_array`/`write_array` require **at least 2** indices (`split_common` needs 2+ paths to find
  the divergence); a single-index "multi" request isn't supported (use the normal non-multi call/read/write).
- Multi requests only work with `PathKindOwned::Absolute` paths (not `GlobalCompact`/`GlobalFull`) — matches the
  original `MultiRead`'s restriction too, not a new gap.

## What's left (tasks #3 and #4 in TaskList — check `TaskList` at the start of the next turn, it should still show
these two as `pending`/`in_progress` since this file was written mid-task #3)

1. **FEATURES.md**: CLT-15 is still 🚧 (search for `CLT-15` in `FEATURES.md`, around the `CLT` section). Flip to
   ✅ now — the three missing bullets it lists (client API for multi write/call, deferred-in-multi refusal,
   full multi-property support) are now all done except "full multi-property support" — check what that phrase
   meant originally (it's about *property arrays* vs the plain properties tested here; `ch[]` in `properties_api`
   covers it, so it should be fine to mark done, but re-read the bullet's exact wording first and adjust if it
   meant something more specific).
2. **docs/api/properties.md** (around L161-162, already mentions the old 2-tuple-only `multi_read`): rewrite to
   describe the generalized tuple API (any arity, any depth, `MultiCall`/`MultiWrite` too) and mention
   `read_array`/`write_array`. **docs/api/methods.md**: no multi-request mention at all yet, add one similar in
   spirit (point at `call_array`, tuple `MultiCall`). Don't run `just build-docs` without asking first (it can break
   a `just serve-docs` instance elsewhere) — just get the prose right, a docs build check isn't required before
   `tpm work ready` for this task (no site-wide link changes are being made).
3. **CHANGELOG double-check**: root `CHANGELOG.md` `[Unreleased]`, `wire_weaver_client/CHANGELOG.md`,
   `ww_client_server/CHANGELOG.md`, `shrink_wrap/CHANGELOG.md`, `ww_self/CHANGELOG.md` already have entries (from
   commits 1, 2, 4 above) — just skim them once more for accuracy against the final shipped shape (e.g. the
   dynamic-client mention, the `next_with`/`des_shrink_wrap_field_dyn` additions) before calling it done; no new
   entries should be needed, this is a final-accuracy pass, not new writing.
4. **`just test` and `just lint` across the whole workspace** — only `cargo check --workspace` and targeted
   `cargo nextest run -p <crate>` / `cargo clippy -p <crate>` have been run so far (all green), never the actual
   `just` recipes end to end. Run both, fix anything `just lint` flags that targeted clippy runs didn't catch
   (e.g. `cargo fmt --check`, `cargo sort -w -g --check`, `typos`, `--all-features` clippy across crates not
   touched directly here but that might pull in changed code, rustdoc warnings on the new `pub` items in
   `multi.rs`/`dynamic.rs` — they have doc comments already but double-check rustdoc `-D warnings` is happy with
   the intra-doc links like `[split_common]`, `[read_array]`, `[EitherAnyVecIter::next_with]` etc. across crate
   boundaries, those sometimes need the full path or `#[allow]`).
5. Once 1-4 are green: `git rm HANDOFF.md`, commit, then `tpm work ready --note "<one line>"` and stop (per the
   top-level session instructions — don't land, push, or run `tpm work done`).

## Files touched (for orientation, not exhaustive — `git log --stat` on this branch is authoritative)
- `wire_weaver_client/src/client/multi.rs` (new, replaces deleted `multi_read.rs`), `prepared_read.rs`,
  `prepared_call.rs`, `prepared_write.rs`, `dynamic.rs`, `event_loop/commander.rs`, `lib.rs`.
- `ww_stdlib/ww_client_server/src/multi_req.rs` (`MultiIndexOwned::iter_u32`/`len`/`is_empty`).
- `shrink_wrap/shrink_wrap/src/either_any_vec.rs` (`EitherAnyVecIter::next_with`).
- `ww_self/src/value.rs` (`ValueOwned::des_shrink_wrap_field_dyn`).
- `tests/properties_api/src/lib.rs` (+`Channel`), `tests/methods_api/src/lib.rs` (+`Unit`), `tests/properties/src/lib.rs`,
  `tests/methods/src/lib.rs`, `tests/dynamic/src/lib.rs` (new `multi_requests` test + `multi_req = true`).
- CHANGELOGs: root, `wire_weaver_client`, `ww_client_server`, `shrink_wrap`, `ww_self` (all done already).
- Still to edit: `FEATURES.md`, `docs/api/properties.md`, `docs/api/methods.md`.
