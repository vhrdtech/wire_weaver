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

## Forwarding

A forwarder sends what crosses a slot on to the backend and back without knowing the mounted API: it rewrites the
envelope only (seq, priority, context, path, kind), and copies arguments, values and stream data as bytes. Seqs pass
through unchanged, since each front connection has a backend connection of its own. There is no wire change: the
backend sees an ordinary request at its own absolute path, the client an ordinary event at the front's.

```rust
use ww_client_server::mount::MountPrefix;

fn hub_request(&mut self, cx: &mut Context<'_, impl EventOut>, request: MountRequest<'_, '_>) -> MountAnswer {
    // in: the request with the path below the slot, everything else as it arrived
    let bytes = request.forward_into(&mut self.buf)?;
    // remembered per slot: the path of its `api`, [hub, 1] here
    let prefix = MountPrefix::new(request.prefix()?);
    ..
}
// out: each event of the backend, its path (stream data, sideband, stream items, property updates) with the
// prefix in front; replies, writes and errors carry no path and are copied as they are
let event = prefix.event_out(&backend_event, &mut buf)?;
```

`ww_client_server::mount::request_at` writes any request at another absolute path, `request_bytes_at` the same from
bytes. A message that doesn't decode or doesn't fit the buffer is a `RewriteError`.

### Global paths into a mount

A global trait path (`GlobalFull`, `GlobalCompact`) names a trait wherever it sits in the device's tree. A server
knows the traits of a mounted API only from the backend's introspection data, learned when the slot comes `Up`.
`wire_weaver_server::mount::MountedTraits` holds them: `mount(api_path, bundle)` at `Up`, `unmount(api_path)` at
`Absent`, and `request_into_mount(request, buf)` rewrites a request whose global path reaches into a mounted API to
the front's absolute path into the slot, which the generated dispatch then routes to the slot's handler.

- The server's own tree comes first: a trait it has resolves as without mounts (by its dispatch, or by the client
  from its introspection data), and mounting a backend never moves it.
- Then the slots, in path order: depth first, in id order, the elements of `mounts!` by index.
- Inside a mounted API, a trait is found as its own server finds it: the first one depth first, never under a
  resource array, in a protocol compatible version of its crate (`ww_global` id for `GlobalCompact`).

## Client

The generated client hands out a `wire_weaver_client::MountSlot`:

```rust
let hub = device.hub()?;
let presence = hub.presence().read().await?;          // MountPresenceOwned
let api_path = hub.api_path();                         // path of the mounted API's root
let ext = device.ext_by(&name).await?;                 // Option<MountSlot>, by MountNameOwned
let first = device.ext(0)?;
```

### Views of a mounted API

A `MountView` is how a client works with the API mounted at a slot. Its `commander()` is a `Commander` whose paths
are in the mounted API: each request goes out with the slot's `api` in front, and each answer, event and stream item
comes back under the same path. The view holds its own facts of the mounted API, apart from the device's: crate,
version, hashes, introspection data (asked for below the slot's `api`), global trait paths, `const` values and keys.

```rust
// a generated client of the mounted API
let view = tv.hub()?.view_for(&hub_api::Client::client_line()).await?;
let hub = hub_api::Client::on_mount(&view)?;
hub.player().pause().call().await?;

// the dynamic client
let view = tv.hub()?.view().await?;
let info = view.commander().device_api_info();           // the mounted API, not the TV's
```

- `view_for` checks calls against the mounted API as a connect checks them against a device's API.
  `on_mount` refuses a view made without the client's line. `view()` is for the dynamic client and tools.
- The view follows the slot's `presence`, observed on the link, while one of its clones lives. When a backend
  comes `Up`, the view learns its API. Another hash later means another revision, learned again as a reconnect
  learns a device's (CLT-81): the view's kept streams, sinks and observers that don't hold on it end.
- After a link gap, the view resumes its streams, sinks and observers on the first `Up`. The device's own API is
  not relearned.
- `status()` is a `watch` of the presence the view handled last. It says `Up` only once the API is learned. It
  doesn't change while the link is down: `Commander::connection_state` tells that.
- A global trait path resolves in the mounted API's introspection data. One the view can't resolve is refused:
  sent as it is, the device would look for the trait in its own tree first.
- A request into an empty slot fails with `Unavailable`.

### Walking into mounts

`DynResource::walk_mounted(cmd, path)` walks the dynamic client into mounted APIs. It returns the resource and the
view it lives in (`DynWalked`); use the resource with `walked.cmd(cmd)`.

- `hub.api.player.pause` and `hub.player.pause` name the same item: the slot's `api` may be left out.
- `hub.presence` stays the slot's own item; `hub.api.presence` reaches an item of the mounted API named so.
- A name after a `mounts!` array picks a slot by name: `ext.cast.pause` is `ext[MountName("cast")].pause`.

`ww observe` and `ww stream` walk this way.

Tests: `tests/mounts` (both server kinds, the generated and dynamic clients, views and walks,
`forward_to_a_backend`: the hub forwarding to another TV through the rewrite), `tests/mounts_api`. Conformance cases:
`api.mount.presence.absent`, `api.mount.presence.up`, `api.mount.name`, `client_server.error.unavailable`.
