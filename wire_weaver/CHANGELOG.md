## Unreleased

### ⚠️ Breaking

- `WireWeaverAsyncApiBackend::process_bytes` takes `out: &mut EventWriter<'_, impl MessageSink>` and
  `medium: Self::Medium` instead of `sink: &mut impl MessageSink`, `WireWeaverApiBackend::process_bytes` takes
  `out: &mut impl BlockingEventOut` and `medium`. Both traits have a `type Medium: Copy` (`()` unless the server is
  generated with `medium = ".."`). Forward both to the generated `process_request_bytes(data, scratch, out, medium)`,
  for a `use_async = false` server on an async event loop through `out.queued(|q| ..).await`.
- `defmt-extended` and `tracing-extended` features removed together with the ones in `shrink_wrap`; drop them from
  your `Cargo.toml`. `wire_weaver` no longer depends on `tracing`.

### 🚀 Features

- `Context<'_, O, M = ()>`, passed to every server handler: request `seq()`, `medium()` the request came from,
  `reply_to()` (`ReplyTo { medium, seq }`) to answer a deferred call later, and it sends events (stream updates,
  property updates, deferred replies) through `EventOut` (async) / `BlockingEventOut` (sync).
- `EventOut::send_with()` / `send_event()` and `BlockingEventOut::send_with_blocking()` / `send_event_blocking()`
  serialize an event into a scratch buffer and send it, `SendError` tells why it failed.
- `EventWriter { sink, scratch }` implements both for a `MessageSink` / `BlockingMessageSink` (new sync variant of
  `MessageSink`). `EventWriter::queued()` runs sync code that sends events from async code: they are queued in the
  scratch buffer (`EventQueue`) and sent after it returns.
- `api_id` module: `ww:<crate>@<version> h=<hash>[ l=<label>]` identity string for USB interface descriptors, so
  hosts can identify a device without opening it. `api_id_string!` builds it at compile time, `with_label()` appends
  a runtime user label within the 126-character USB string limit, `parse()` reads it back (all `no_std`, no alloc).
- `From<Unimplemented>` for `RpcResult`, `SetResult` and `GetResult` instead of `Into`, the `Into` direction still works.

## [0.4.0] - 07 Jan 2026

### 🚀 Features

* Re-export:
    * shrink_wrap
    * wire_weaver_derive
    * ww_version
