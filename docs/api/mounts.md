---
level: advanced
---

# Mount slots

A mount slot is a place in an API whose own API is known only at run time: a TV whose home hub runs in another
process, or extensions that any local program can register. The parent API declares the slot; whatever is mounted
there brings its own introspection data, API hash and version.

This differs from [`ww_impl!`](local_trait.md#nested-levels), whose nested level is a fixed type known at compile
time, and from [`serve_trait`](local_trait.md#serving-a-local-implementation), which composes such levels in one
process: a mount slot's backend is a separate process, unknown until it connects, with its own hash and version.

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
    let prefix = MountPrefix::from_vec(request.prefix()?);
    ..
}
// out: each event of the backend, its path (stream data, sideband, stream items, property updates) with the
// prefix in front; replies, writes and errors carry no path and are copied as they are
let event = prefix.event_out(&backend_event, &mut buf)?;
```

`ww_client_server::mount::request_at` writes any request at another absolute path, `request_bytes_at` the same from
bytes. A message that doesn't decode or doesn't fit the buffer is a `RewriteError`.

### Forwarding in firmware

The rewrite needs no heap: `MountPrefix<S>` keeps the slot's path and the joined path of the last event in a
`PathStore`. A host uses a `Vec` (`MountPrefix::from_vec`, no limit of its own), a firmware a `PathArray<N>` of at
most `N` ids, in a `static` with the forwarder's other state. Both run the same code and give the same bytes; a
joined path longer than `N` is `RewriteError::PathTooLong`, never a cut path.

```rust
use ww_client_server::mount::{MountPrefix, PathArray};

// 8 ids in all: the slot's path and the path below it
let mut prefix = request.prefix_in(PathArray::<8>::new())?.ok_or(NoPrefix)?;
let event = prefix.event_out(&backend_event, &mut buf)?;
```

The cost is decoding the backend's `Event`, measured with `examples_mcu/mcu_qemu`'s `qemu-mount-prefix` on
thumbv7m (QEMU, stack painted below the call). A decoded `Event` is 100 B: a `Series` in it keeps only where its
columns start (shrink_wrap SW-87). Rewriting an observe update takes 648 B of stack at `fw-z` (556 B at `fw-3`) and
about 2 300 instructions (1 600). An update with `sample_times` adds the validation of that `Series`: 3.5 KB of stack
at `fw-z` (2.7 KB at `fw-3`) for three `Second`s, most of it the series contexts of `NumericValue`'s variants
(shrink_wrap SW-88). A firmware forwarder needs that stack in the task that rewrites.

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

### Backend processes

`wire_weaver_server::mount` does all of the above for a backend that is another WireWeaver server reached over a
byte stream (a local socket, TCP, a pipe), with no tokio needed: it runs on any executor, a calloop loop included.

- `Mounts` is the host's mount table, shared by every connection. `mount(api_path, connector, MountOptions)` adds a
  slot, `unmount` removes it. `run(clock)` keeps a control link to each backend: when it comes up, the backend's
  introspection data is learned into `MountedTraits` and `presence` turns `Up` with the backend's API, version and
  hash; when it goes, `presence` turns `Absent` and the link is tried again after `MountOptions::retry_after`.
- `Server::mounts(mounts.routes())` reroutes a global path into a mounted API as the request arrives, before the
  authorization hook sees it.
- `Forwarder` belongs to one front connection and opens that connection's own link to each backend it uses. The
  slot handler calls `forward(cx.call(), &request)`; the backend's `wait`, `next_wake`, `poll`, `cancel` and `end` call the
  forwarder's.
- `Forwarder::poll` sends the backend's replies and events with the slot's path in front, and returns the
  `MountEvent`s (a slot mounted, its presence changed, unmounted): the backend then calls
  `mark_changed_<slot>_presence()` and, for `mounts!`, `ext_keys_send(..)`.
- `Connect` opens the byte stream: `LocalSocket` on tokio (feature `local_socket`), `ww_calloop::UnixConnector` on a
  calloop loop.

```rust
// once per host
let mounts = Mounts::<LocalSocket>::new(own_bundle);
mounts.mount(&hub_path, LocalSocket::new("/run/user/1000/hub.sock"), MountOptions::default());
let server = Server::new(clock).mounts(mounts.routes());
let table = mounts.clone();
spawn(async move { table.run(clock).await });

// per connection
fn hub_request(&mut self, cx: &mut Context<'_, impl EventOut>, request: MountRequest<'_, '_>) -> MountAnswer {
    self.forwarder.forward(cx.call(), &request)
}
```

A client cancelling a forwarded request reaches the backend through `Backend::cancel`. When the backend goes, each
request it didn't answer gets `Unavailable` (`err_seq::MOUNT_BACKEND_GONE`), and each stream opened and each trait
level observed through the slot a `Close` sideband at its path: the client's stream ends, its observers get
`ObserveEvent::Ended`. A request past 64 waiting for the link or 256 in flight is refused with `Overloaded`
(`err_seq::MOUNT_QUEUE_FULL`); a request longer than the backend takes, or a reply longer than the front's link
takes, with `ResponseSerFailed` (`err_seq::MOUNT_TOO_LONG`).

What is trusted: the mount table (the host's own configuration) and whoever listens on the backend's socket
(`LocalSocket` refuses a process of another user). The backend's messages are not: only their envelope is decoded,
a reply passes only under a seq sent on that link, events get the slot's path in front, and introspection data is
capped at 4 MiB.

### Modules in the firmware's process

A firmware can serve a slot from code in its own process: a module. The module may be a wasm module run by an
interpreter, or a WireWeaver server linked into the firmware. Nothing crosses a link: the slot handler calls the
module with the request as bytes, and sends the module's replies and events to the client with the slot's path in
front. `ww_device::mount` does this without a heap.

- `Module` is the trait a module implements. `model()` returns the module's model, `process(request, events, reply)`
  serves one request at the module's own absolute path, and `poll(events)` gives the module a turn outside of a
  request, for stream items, observed property updates and deferred replies. Everything is bytes, so a wasm host
  implements it from its host calls.
- `Linked<S>` turns a server generated with `use_async = false` into a `Module`: the server and its
  `WW_SELF_BYTES_NO_DOCS`.
- `ModuleSlot<M, MSG, OUT>` holds one module for one front connection, with every buffer it needs. `MSG` is the
  longest message in and out, normally the front link's message limit. `OUT` is the room for the module's events in
  one turn.

The module's model is its server's introspection data without docs and uncompressed: the `WW_SELF_BYTES_NO_DOCS`
constant that every generated server has. A constant takes flash only where it is used. The slot reads the crate, version and wire version
from the model once, at `load`, and hashes the model itself. That hash is the server's own `API_HASH_NO_DOCS`, so a
generated client of the module's API takes the slot's API as known. A client that sees a new hash after a reload
relearns this slot's API only; the parent's hash and the other slots stay as they were.

```rust
use ww_device::mount::{Linked, ModuleSlot};

// in .bss, built in place: nothing of it on the stack
static HUB: StaticCell<ModuleSlot<Linked<HubServer>, 128, 256>> = StaticCell::new();
let hub = ModuleSlot::init(HUB.uninit());
hub.load(Linked { server: HubServer::new(), model: &hub_impl::WW_SELF_BYTES_NO_DOCS })?;

// the parent's handlers
fn get_hub_presence(&mut self, cx: &mut Context<'_, impl BlockingEventOut>)
    -> GetResult<MountPresence<'_>, CommonError<'_>> {
    Value(self.hub.presence())
}
fn hub_request(&mut self, cx: &mut Context<'_, impl BlockingEventOut>, request: MountRequest<'_, '_>) -> MountAnswer {
    self.hub.forward_blocking(cx, &request)
}

// the parent's event loop
hub.poll_blocking(&mut out);
if hub.take_changed() {
    server.mark_changed_hub_presence();
}
```

An async parent calls `forward` and `poll` instead. `load` replaces the module loaded before and returns it, and
`unload` takes it out. Either way the old module's requests are answered `Unavailable` and its streams and
observed levels get a `Close` sideband, as when a backend process goes. `presence` follows: `Up` with the module's
API while one is loaded, `Absent` otherwise, and `take_changed` says when the backend should call
`mark_changed_hub_presence()`. A `static` of `ModuleSlot::new()` works too, but it is placed in `.data`, so its whole
size also takes flash: the padding of its fields keeps it out of `.bss`.

#### What the slot checks

A loaded module is untrusted code. The slot checks everything the module says before it reaches the parent's
`EventOut`, and a misbehaving module never makes the parent panic:

- The model must decode as a `ww_self::ApiBundle` that names a crate for its root, with its crate name, pre-release
  and build metadata within 64 bytes (`IDENTITY_TEXT_MAX`). Otherwise `load` refuses the module and the slot stays as
  it was. The module's word on its hash is never taken.
- Each event must decode as an `Event`. A reply, or stream data of a request, passes only under a seq the slot sent
  the module and the module hasn't answered yet.
- An event with a path gets the slot's path in front, so it can't reach the parent's own tree. A joined path past
  16 ids (`PATH_IDS_MAX`), or a message that doesn't fit the slot's buffers, is dropped.
- A reply the module writes outside of the buffer it was given, or a `ModuleError::Fault` (a wasm trap, a module out
  of fuel), unloads the module.
- A reply that doesn't decode is answered `ResponseSerFailed` under its request's seq.
- More than 8 requests in flight (`IN_FLIGHT_MAX`) are refused `Overloaded`, a request longer than `MSG`
  `ResponseSerFailed`.

`ModuleSlot::counters` counts what was dropped or refused (`ModuleCounters`): malformed events, stray replies, events
that didn't fit, messages the parent's `EventOut` refused, refused requests, lost answers and faults. The module
never sees a request's context: the slot clears it, so a client can't tell a module who it is.

A module's effects all pass for now: the slot doesn't check what a module may do. Per-module effect caps (TR-24)
will be checked in `ModuleSlot::forward`, before the module sees the request.

#### Global paths into modules

A global trait path (`GlobalFull`, `GlobalCompact`) reaches a module's traits as it reaches a backend process's, in
the same order: the firmware's own tree first, then the slots in path order, inside a module the first trait depth
first, never under a resource array, in a protocol compatible version of its crate.

- `ModuleSlot::load` reads the module's traits from its model once, into a table of fixed size in the slot: reaching
  an item of the model decodes everything before it, so a walk per request would cost a model decode each time
  (about 69 000 instructions and 8 KB of stack in the bench below).
- A server with mount slots gets a generated `has_global_trait(&PathKind)`, which says whether its own tree has the
  trait, and the `api` paths of its slots: `HUB_API_PATH` for a slot `hub`, `ext_api_path(index_chain)` for an
  element of `mounts!`.
- The backend's `global_into_mount(request, buf)` (`WireWeaverApiBackend`, `WireWeaverAsyncApiBackend`) rewrites a
  request into a module. `ww_device`'s `Server` and `blocking::Server` call it before dispatch; the generated dispatch
  then routes the rewritten request to the slot's handler. `ModuleTraits` does the rewrite with no heap.
- A request with an absolute path is only peeked at, not decoded: the hook costs every request about 500
  instructions.

```rust
fn global_into_mount<'b>(&self, request: &[u8], buf: &'b mut [u8]) -> Option<&'b [u8]> {
    ModuleTraits::<1>::new()
        .slot(&api_impl::HUB_API_PATH, &self.hub)
        .request_into_mount(request, api_impl::has_global_trait, buf)
}
```

A generated client resolves a trait of the device's own tree from the device's introspection data and sends its
absolute path, so the hook mostly sees global paths into modules.

#### Limits and cost

- A module holds at most 16 traits (`MODULE_TRAITS_MAX`) whose paths take 48 ids together
  (`MODULE_TRAIT_IDS_MAX`), each at most 14 levels deep (`MODULE_TRAIT_DEPTH_MAX`). A module past them is refused at
  `load` (`LoadError::TooManyTraits`, `TraitsTooDeep`), never served with part of its traits.
- `GlobalCompact` doesn't reach a trait of a dependency crate that the module's model leaves out (it carries no
  `ww_global` id there): `GlobalFull` does.
- One `ModuleSlot` serves one front connection.

`examples_mcu/mcu_qemu`'s `qemu-mount-module` measures a slot with `mount_backend_api`'s hub, `MSG` 128 and `OUT`
256, on a Cortex-M3 under QEMU. The table gives instructions, not cycles, and the stack painted below each call.

| Step                                        | `fw-z` instructions | `fw-z` stack | `fw-3` instructions | `fw-3` stack |
|---------------------------------------------|--------------------:|-------------:|--------------------:|-------------:|
| `load` (decode model, trait table, hash)    |              99 565 |      5 240 B |              91 559 |      5 736 B |
| `presence`                                  |                 148 |         64 B |                 132 |         64 B |
| `forward_blocking` of a call                |               4 537 |      1 672 B |               2 965 |      1 304 B |
| `forward_blocking` of a stream open         |               5 531 |      1 672 B |               3 487 |      1 304 B |
| `poll_blocking`, one item                   |               3 058 |        752 B |               1 818 |        580 B |
| `global_into_mount`, an absolute path       |                 576 |        528 B |                 467 |        504 B |
| `global_into_mount`, `GlobalFull` to `Lamp` |               4 854 |      1 056 B |               3 481 |        968 B |

The slot takes 2 920 B of RAM at these sizes, 584 B of it the trait table. Most of `load`'s stack is the decode of
the borrowed `ApiBundle`. The whole bench is 76 750 B of flash at `fw-z`: the slot's code about 12 KB, the SHA-256
of the model hash 7 KB, decoding the model at `load` 8 KB. Global paths add about 9 KB: 3.2 KB of it decodes the
model's items, 1.1 KB a `Request`, which a firmware's dispatch has already.

### Who calls and what it does

A call into a mount is checked twice: by the front server's [authorization hook](host_server.md#who-may-call) before
it is sent on, and by the backend's before it runs.

The front's hook sees the whole way, also below the slot. The steps of `Resource::items` after the slot's `api` are
the mounted API's items, with the effects its backend declares (`#[effect(..)]`, [metadata](metadata.md)), taken from
the API learned when the backend came up. `Resource::mount` says the request goes into a slot: `api_path` is the path
of the slot's `api`, `api` the learned API. A slot's API is learned before its presence turns `Up`. When none could be
learned (introspection off, longer than 4 MiB), `api` is `None` and so are the items: nothing is known of what the
request does, and a policy that limits effects refuses such a request.

```rust
let policy = from_fn(|cx: &CallContext, resource: &Resource<'_>| {
    let unknown = resource.mount.is_some_and(|mount| mount.api.is_none());
    if unknown || resource.effects().contains(Effects::DESTRUCTIVE) {
        return Err(Denied);
    }
    Ok(())
});
let server = Server::new(clock).mounts(mounts.routes()).authorize(policy, Some(own_bundle));
```

The backend learns which client of the front called. The forwarder sends the peer the front's transport saw
(`cx.call().peer`: a unix user, a key, an address, or `Unknown`) in each request's context,
`RequestContext::forwarded_for`, and replaces whatever the client put there. A call from the front's own process
names nobody. The backend takes that word only from a peer it trusts to forward:

```rust
// the hub: its socket lets in only its own user, and the TV runs as that user
let server = Server::new(clock).trust_forwarders(|peer| matches!(peer, Peer::Unix { .. }));
```

Then its handlers and its own hook see the front's client as `cx.call().peer`, with `cx.call().forwarded` set; the
connection's peer (`Connection::peer`) stays the forwarder. Without `trust_forwarders`, or from any other peer, the
name is ignored and the request is the forwarder's. A trusted peer may name any client, so trust only the forwarder.
A trusted forwarder's request whose context doesn't decode is `Unknown`'s, never the forwarder's own.

What each side relies on:

| Side | Trusts | A hostile one can |
|---|---|---|
| Front | its mount table, the peer listening on the backend's socket | (backend) misstate the effects of its own items, so the front's hook judges its calls by them; it reaches nothing of the front's own tree |
| Backend | the forwarders `trust_forwarders` admits | (trusted forwarder) name any client for any request; (any other peer) nothing: its names are ignored |

Every effect passes a mount: the host limits a mounted API's calls through its hook. Network peers are known by
their address only until peer authentication (TR-24, planned) lands.

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
