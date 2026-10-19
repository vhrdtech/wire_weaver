---
level: advanced
---

# Sync and async handlers

A server generated with `use_async = true` expects every user handler to be an `async fn`, and with
`use_async = false` every handler to be a plain `fn`. Most handlers of a device never wait for anything (they read a
field, set a pin, answer `Unimplemented`), and an `async fn` costs flash even when it never awaits: each call
becomes a state of the dispatcher's future. On Donguru (Cortex-M0+), ~115 of ~130 handlers never await; making them
plain `fn`s saved 20.2 KiB of flash at `opt-level = "z"` (282 -> 262 KiB) and 12.6 KiB at 3.

`handler_model` lists the exceptions, so an async server can have sync handlers (SRV-21):

```rust
mod server_impl {
    wire_weaver::ww_codegen!(
        donguru_api :: DonguruApi for super::ServerState,
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
