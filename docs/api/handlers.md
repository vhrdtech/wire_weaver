---
level: advanced
---

# Sync, async and shared handlers

A server generated with `use_async = true` expects every user handler to be an `async fn`, and with
`use_async = false` every handler to be a plain `fn`. Most handlers of a device never wait for anything (they read a
field, set a pin, answer `Unimplemented`), and an `async fn` costs flash even when it never awaits: each call
becomes a state of the dispatcher's future. On Portwright (Cortex-M0+), ~115 of ~130 handlers never await; making them
plain `fn`s saved 20.2 KiB of flash at `opt-level = "z"` (282 -> 262 KiB) and 12.6 KiB at 3.

`handler_model` lists the exceptions, so an async server can have sync handlers (SRV-21):

```rust
mod server_impl {
    wire_weaver::ww_codegen!(
        portwright_api :: PortwrightApi for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        // these await (flash writes, ADC reads, a VBUS cycle), all others are plain `fn`s
        handler_model = "device.settings.*|meter.reading|plug.vbus_cycle=async, _=sync",
    );
}
```

- Each pattern is a regex matched against an item's whole dotted path in the API tree (`device.settings.vbus_mode`
  for the `vbus_mode` property of the `settings` trait under `device`); the first one that matches decides. `.`
  matches the dot between levels too, so `device.settings.*` takes everything under `device.settings`.
- `_` is the default for the items no pattern matches. Without `_`, they follow `use_async`; without
  `handler_model`, every handler does, as before.
- A pattern can't contain `,`, `=` or whitespace. A pattern that matches no method, property or stream is a
  compile error (a typo, or an item that was renamed), and so is `async` in a server with `use_async = false`.

The model is per item: every handler of an item is the same kind. For a property that is its getter, setter and
`changed_<name>`, for a stream its open handler, `close_<name>`, `sideband_<name>` and the `stream_<name>()` state
accessor, for a sink `write_<name>` and `sideband_<name>`. `valid_indices_<..>` and `observe_state()` are always
plain `fn`s.

## What the generated server does with it

- The dispatcher calls a sync handler without `.await`, and `poll_properties` reads a sync getter without it.
- A trait level whose items are all sync, down to its leaves, gets a plain `fn` dispatcher (`process_<level>`),
  called without `.await` from the level above. Some items await on their own whatever their handlers are: a reliable
  stream (its store is async), a credit sink (it sends credit events), and the root level when it serves
  introspection.
- `process_request_bytes` and `poll_properties` stay `async fn` in an async server.

A sync handler in an async server still gets `cx: &mut Context<'_, impl EventOut, ..>`, but can't await a send on
it: a handler that sends events (a deferred answer right away, a stream item) stays `async`.

## A handler written the other way

Every call goes through `wire_weaver::handler::sync(..)` or `wire_weaver::handler::r#async(..)`, which compile to
nothing but check the kind: a handler written as `async fn` where the model says sync is a compile error, not a
future that is dropped without running, and the other way round.

```text
error[E0277]: this handler is `async fn`, but `handler_model` (or `use_async = false`) makes it a plain `fn`
    = note: make the handler a plain `fn`, or list it as `async` in `ww_codegen!(.., handler_model = "..")`
```

The error points at the `ww_codegen!` call, and names the handler's output type; `debug_to_file` shows which call
it is.

## Scaffolding

`ww api scaffold <api_crate> --use-async --handler-model "<model>"` writes `async fn` or `fn` per item to match, and
passes the model on to the `ww_codegen!` call it prints. To change an existing server, set `handler_model`, then let
the compiler list the handlers that are now the wrong kind.

## Shared and exclusive handlers

A host server can serve several requests of a connection at the same time: a slow call doesn't hold up a quick
read. Two requests can only run side by side if neither changes the server, so their handlers take `&self`.
`receiver_model` says which items are such (SRV-65):

```rust
mod server_impl {
    wire_weaver::ww_codegen!(
        sensor_api :: Sensor for super::Server,
        server = true, no_alloc = false, use_async = true,
        receiver_model = "status|sensors.*|channel.level=shared",
    );
}
```

- Every item is `exclusive` unless the model says `shared`; `_=shared, motor.*=exclusive` turns it around. The
  patterns work as in `handler_model`, and a pattern that matches no shareable item is a compile error.
- Only calls and reads can be shared: a method, a readable property's getter, a key companion's `index_of` and
  `key`. A property's setter, streams, sinks, observing and `const` items stay exclusive whatever the model says.
- A shared item's handler takes `&self`. So does the `valid_indices_<..>` of every array on its path, and the one of
  a keyed array whose `key` is shared. State a shared handler changes sits behind its own lock or atomic.
- A server that sets no `receiver_model` is generated as before: every handler takes `&mut self`.

With a model, the server gets a second entry beside `process_request_bytes`:

| Generated                                   | Takes          | Serves                                              |
|---------------------------------------------|----------------|-----------------------------------------------------|
| `process_request_bytes(&mut self, ..)`      | every request  | everything, shared items included, alone             |
| `process_shared_bytes(&self, ..)`           | shared ones    | calls, reads, multi calls and multi reads of shared items |
| `is_shared(bytes) -> bool`                  | request bytes  | which of the two entries a request goes to           |

`is_shared` takes no `self`: a runtime asks it while an exclusive request holds the server. It says yes to a `Call`,
`Read`, `MultiCall` or `MultiRead` of a shared item, on an absolute or a global trait path, and never to a request
with `seq` 0. The shared entry answers anything else with `OperationNotSupported` at the request site
`err_seq::REQUEST_NOT_SHARED`. The two entries share the level tree's code generation, so a shared request is
answered with the same bytes through either. A deferred method can be shared: it keeps its `ReplyTo` behind a lock
and answers later with `<method>_send_return`, as before.

`process_request_from` and `process_shared_from` take the connection's `origin: &CallContext` (its peer and
connection id) before the bytes; handlers read it from `cx.call()`, with the request's own `seq` and `priority`. The
plain entries pass a peer the transport doesn't identify. A host runtime calls them through
`wire_weaver::WireWeaverSharedApiBackend`.

A firmware doesn't call the new entries. They are generic over the event sink, so one that isn't called is never
compiled in, and `process_request_bytes` is the same code as without a model.

A shared item whose handler takes `&mut self` doesn't compile: the error points at the `ww_codegen!` call.

```text
error[E0596]: cannot borrow data in a `&` reference as mutable
   = `self` is a `&` reference, so it cannot be borrowed as mutable
```

`ww api scaffold <api_crate> --receiver-model "<model>"` writes `&self` where the model needs it and passes the model
on to the `ww_codegen!` call. `tests/shared_handlers` has a blocking `no_alloc` and an async server with every path
shape: levels, arrays, a keyed array, multi requests, a deferred method and a global trait path.
