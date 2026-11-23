---
level: advanced
---

# Mount slots

A mount slot is a place in an API whose own API is known only at run time: a TV whose home hub runs in another
process, or extensions that any local program can register. The parent API declares the slot; whatever is mounted
there brings its own introspection data, API hash and version.

```rust
#[ww_api_root]
#[effect(edit)]
trait Tv {
    /// The home hub, when its process runs.
    mount!(hub: hub_api);
    /// Extensions by name: any local program can be mounted.
    mounts!(ext);
    fn mute();
}
```

- `mount!(hub)` is one slot. `mount!(hub: hub_api)` also names the API crate a backend is expected to serve: a hint
  for tools, not checked on the wire.
- `mounts!(ext)` is an array of slots keyed by name (`ww_api::mount::MountName`, at most 64 bytes), as
  [`keyed!`](arrays.md#keyed-arrays) keys an array: it takes two ids, the array `ext` and its key companion
  `ext_keys` (`index_of`, `key[i]` and the `keys` stream of `ww_keyed::KeyEvent` that announces `Added` and
  `Removed` slots). The API crate depends on `ww_keyed` for it.
- `#[optional]` and `#[errors(..)]` are refused on a slot: a slot is always there, only empty, and the mounted API's
  items have their own errors.

## What a slot holds

Every slot is a fixed level of two items, the same for `mount!` and each element of `mounts!`:

| Id | Item       | What it is                                                                                      |
|----|------------|-------------------------------------------------------------------------------------------------|
| 0  | `presence` | read-only, observable `ww_api::mount::MountPresence`: `Absent`, or `Up { api, hash, wire }`      |
| 1  | `api`      | the root of the mounted API: a request below it goes to the backend with the slot's path taken off |

`Up` carries the mounted API's crate and version (`FullVersion`), the hashes of its introspection data
(`ApiHashPair`) and its wire version. A client relearns the slot's API only when the hashes change.

The parent API's model holds the declaration only: one `Mount` item per slot in `ww_self`, with the expected crate
and the presence type. Mounting another backend never changes the parent's hash. An `Introspect` request at a slot's
`api` asks for the mounted API's own introspection data.

## Server

`ww api scaffold` prints the handlers. For `mount!(hub)`:

```rust
fn get_hub_presence(&mut self, cx: &mut Context<'_, impl EventOut>)
    -> GetResult<MountPresenceOwned, CommonError<'_>>;
fn hub_request(&mut self, cx: &mut Context<'_, impl EventOut>, request: MountRequest<'_, '_>) -> MountAnswer;
```

The elements of `mounts!(ext)` take `index_chain` like any array, plus `valid_indices_root_ext`, `ext_index_of` and
`get_ext_key` of the key companion.

- `presence` is always a getter, whatever the `property_model`. It is observable: `mark_changed_hub_presence()`
  tells observing clients that a backend came or went.
- `<slot>_request` gets every request below the slot's `api`: `ww_client_server::mount::MountRequest` has the
  request as it arrived and `path`, the part below `api` (empty at the mounted API's root). The handler sends it on
  and answers `MountAnswer::Forwarded`; the backend's reply goes to the client under the request's own seq.
- `MountAnswer::Unavailable` answers the request at once with `ErrorKind::Unavailable` (error site
  `err_seq::mount::HANDLER`). A client whose model is older than `ww_client_server` w2.2 gets `UserStr("unavailable")`.
- A multi request below a slot is refused per element with `OperationNotSupported` (`err_seq::mount::IN_MULTI`),
  before the handler runs.
- `stream_data_ser().ext_keys_send(..)` announces a slot added or removed on the `keys` stream.

An authorization hook sees a slot's `presence` as a read and the slot's `api` as the last step, with no effects: the
path below it is the mounted API's, not in the parent's model.

## Client

The generated client hands out a `wire_weaver_client::MountSlot`:

```rust
let hub = device.hub()?;
let presence = hub.presence().read().await?;          // MountPresenceOwned
let api_path = hub.api_path();                         // path of the mounted API's root
let ext = device.ext_by(&name).await?;                 // Option<MountSlot>, by MountNameOwned
let first = device.ext(0)?;
```

`api_path()` is the prefix a view of the mounted API puts before its own paths. The dynamic client and `ww` read
`hub.presence` and `ext[i].presence` by path; walking into `hub.api` says the mounted API is known only at run
time. Working with the mounted API itself (its introspection, its items under the slot) is not built yet.

Tests: `tests/mounts` (both server kinds, the generated and dynamic clients), `tests/mounts_api`. Conformance cases:
`api.mount.presence.absent`, `api.mount.presence.up`, `api.mount.name`, `client_server.error.unavailable`.
