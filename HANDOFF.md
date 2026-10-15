# CLT-15 multi-write-call client — handoff

## Task
Generalize the client-side multi-request API (`MultiRead`/`MultiCall`/`MultiWrite`): today only `MultiRead`
exists, as a hand-written 2-tuple-at-root-only trait impl in `wire_weaver_client/src/client/multi_read.rs`. Need:
N-tuple (not just 2), any depth (not just root), `MultiCall`/`MultiWrite` siblings, homogeneous array requests
(Range/List/Mask32 across an array's indices, `MultiArgs::Same`/`Different`), and a dynamic-client (`DynResource`)
equivalent. Tests in `tests/properties` and `tests/methods`. CHANGELOG + FEATURES `CLT-15` ✅. No version bump
(`wire_weaver` lands with `--no-bump`).

Device side already landed on master (e6fb43c3, 45fd6f1b): `MultiIndexIter` (Range/List/Mask32),
`RequestKind::MultiCall`/`MultiWrite` dispatch, deferred methods refused with `OperationNotSupported` inside a
multi request. Wire format fully understood from `wire_weaver_core/src/codegen/api_server.rs`
(`multi_req_handlers`, ~L588-717) and `tests/multi_req`:
- Request: path (common prefix) + `RequestKind::{MultiRead,MultiCall,MultiWrite}{ multi_idx, in_each_array_id,
  multi_args/multi_data }`. Each index is processed as a `Read`/`Call`/`Write` on `path ++ [index] ++
  in_each_array_id?`.
- `MultiArgs::Same(bytes)` reuses one serialized blob for every index; `Different(bytes)` is a serialized
  `Vec<Vec<u8>>`, one entry per index, in multi_idx order.
- Response: one `EventKind::Value` holding `EitherAnyVec` of `Either<ErrorOwned, T>`, in index order. No length
  prefix — the client must already know how many items to decode (= number of indices requested).

## Design (implemented)
Key insight: both "different resources at one level" (today's 2-tuple MultiRead) and "same resource across an
array" (e.g. call `set_level` on pins 0..4) reduce to the SAME shape: N absolute paths of equal length that agree
on a prefix, diverge at exactly one position (becomes `multi_idx`), and optionally agree again on exactly one
more trailing id after that (becomes `in_each_array_id`). `wire_weaver_client/src/client/multi.rs`
(`split_common`) implements this once and is used by both call shapes below. Same/Different args are chosen
automatically by comparing each item's serialized bytes (all equal → `Same`, saves wire bytes; otherwise →
`Different`) — no separate API needed for that.

1. **Heterogeneous tuple API** (generalizes today's `multi_read()`): `MultiRead`/`MultiCall`/`MultiWrite` traits
   impl'd for tuples via `macro_rules!` for arities 2..=8, generic over the existing generated
   `PreparedRead<T>`/`PreparedCall<T, Error>`/`PreparedWrite<Error>` builders (new crate-internal traits
   `PropertyPath`/`CallPath`/`WritePath` extract path + bytes + commander from each, non-consuming — needs
   `TransportCommander: Clone`, already true). Each tuple position can be any depth, any resource kind, as long as
   all N paths fit the `split_common` shape. No codegen changes needed — this is 100% generic code in
   `wire_weaver_client`.
2. **Homogeneous array API** (Range/List/Mask32 across indices, e.g. `pin[0..4].set_level(true)`): free functions
   `wire_weaver_client::multi::{read_array, call_array, write_array}` (+ `_blocking`), taking the user's own
   `MultiIndexOwned` selector (kept verbatim on the wire — NOT reconstructed as a `List`, so `Range`/`Mask32`
   compactness is preserved) plus a closure `Fn(u32) -> PreparedRead<T>` (etc.) that is just the existing
   generated per-index accessor, e.g. `|i| client.pin(i).read_mode()`. The closure is called once per index in
   the selector purely to extract path/bytes locally (no network traffic) and validate they share the
   `split_common` shape (same prefix + trailing id, varying only at the index position) — then ONE combined
   request is sent with the user's original selector. No codegen changes needed either, since arrays already
   have per-index generated accessors; this just reuses them as a path/bytes source.
3. **Dynamic client** (`DynResource` in `client/dynamic.rs`): `multi_read`/`multi_write`/`multi_call` on a
   `Node::Array` resource, with an optional `child: &str` naming a resource inside the trait the array holds
   (array-of-trait case, e.g. `pin[].set_level`) vs `None` for an array of a flat property/method/stream
   (`property!(led[]: bool)`). Resolves the child's id/types by building a throwaway `self.index(0).child(name)`
   purely for local type lookup (never sent over the wire) and reusing its existing `call_args`/`decode_return`/
   `encode_property`/`decode_property` helpers. For the `child: None` case needs new helpers that read
   `self.item.kind` directly (bypassing `self.kind()`, which always reports `DynResourceKind::Array` for an Array
   node regardless of what the element actually is).

## Files
- `wire_weaver_client/src/client/multi.rs` — replaces `multi_read.rs`: `split_common`, `decode_items`,
  `merge_args`, `PropertyPath`/`CallPath`/`WritePath` traits, tuple macro, `read_array`/`call_array`/`write_array`.
- `wire_weaver_client/src/client/prepared_read.rs`, `prepared_call.rs`, `prepared_write.rs` — leaf trait impls.
- `wire_weaver_client/src/event_loop/commander.rs` — `send_multi_request`/`_blocking` on `TransportCommander`.
- `wire_weaver_client/src/client/dynamic.rs` — `DynResource` multi methods.
- `tests/properties_api`, `tests/methods_api` — add an array sub-trait each (none exist today) to exercise the
  homogeneous path: `properties_api::Channel` (property), `methods_api::Unit` (method).
- `tests/properties/src/lib.rs`, `tests/methods/src/lib.rs` — server handlers for the new array + tests
  (list/range/mask + per-item errors) for both the generated tuple/array API and the dynamic client.
- CHANGELOG: root `[Unreleased]`, `wire_weaver_client/CHANGELOG.md`, maybe `ww_client_server/CHANGELOG.md` if
  `MultiIndexOwned` gains helpers there.
- `FEATURES.md` CLT-15 → ✅.
- `docs/api/properties.md` (already mentions `multi_read`, update), `docs/api/methods.md` (add a short mention).

## Status / progress

Done (both commits build clean, `cargo check --workspace` and `cargo clippy -p wire_weaver_client --all-features`
pass, existing `properties`/`methods`/`multi_req` tests still pass):
- Commit 1 (`feat(client): generalize multi-request client API, any depth/arity (CLT-15)`): everything under
  "Design (implemented)" points 1 and 2 above (tuple macro + array free functions), `ww_client_server`'s
  `MultiIndexOwned::iter_u32`/`len`/`is_empty`, CHANGELOGs (root, `wire_weaver_client`, `ww_client_server`).
- Commit 2 (uncommitted at the point of the soft-stop pause, about to be committed): point 3, `DynResource`
  (`client/dynamic.rs`) gets `multi_read`/`multi_write`/`multi_call` for a `Node::Array` resource, with
  `child: Option<&str>` (array-of-trait vs flat array), `DynMultiArgs`/`DynMultiValue` (`Same`/`Different`).
  `Commander::transport()` is now `pub(crate)` (was private) so `dynamic.rs` can reach `client::multi::send_multi`/
  `decode_items`/`merge_args` (also bumped to `pub(crate)`).

Not started yet (tasks #8-#11 in the session's TaskList):
- No new tests at all yet for anything in this file (tuple N>2, array read/call/write with
  List/Range/Mask32 + per-item errors, dynamic client). `properties_api`/`methods_api` have no array trait today -
  needs adding one to each (see "Design" above: `Channel`/`Unit`), with `multi_req = true` server handlers
  (model closely on `tests/multi_req`'s `Pin` handlers and `tests/multi_req_api`).
- FEATURES.md CLT-15 still 🚧, not yet ✅ - flip it only once tests land (the missing items it lists are now done,
  but flipping before there's a test for each would be premature).
- `docs/api/properties.md` (L161-162, already mentions 2-tuple-only `multi_read`) and `docs/api/methods.md` (no
  mention yet) need updating for the generalized API.
- Blocking + Promise variants of the tuple traits (`MultiRead`/`MultiCall`/`MultiWrite`) don't exist, only async -
  matches the prior `MultiRead`'s shape, not a regression, but worth a mention if the user asks for them.
- `just test`/`just lint` not run for the full workspace yet (only `cargo check`/`clippy` on the touched crates
  and the 3 directly relevant test crates).

Next step for whoever picks this up: tackle task #8 (add `Channel` to `properties_api`, `Unit` to `methods_api`,
with `multi_req = true` server variants in `tests/properties`/`tests/methods`), then #9 (the actual tests), in that
order - the test fixtures are the long pole, the tests themselves are comparatively quick once they exist.
