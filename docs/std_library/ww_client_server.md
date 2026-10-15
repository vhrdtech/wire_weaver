# ww_client_server

`ww_client_server` is the wire protocol every generated server and client speaks underneath a device's own API -
"an API for an API", or a meta-API: a `Request`/`Event` pair that addresses a resource (by path or by trait),
carries a method's serialized arguments or return value as opaque bytes, and leaves what those bytes mean to the
API-specific codegen built on top. It is `no_std` by default (`std` is the crate default; disable it on firmware
with `default-features = false`) and is mostly consumed by `ww_codegen!`-generated code rather than directly,
though some types (`StreamCursor`, `SinkCredit`, the `max_len` helpers) are meant for user code too. Add it with:

```toml
[dependencies]
ww_client_server = { version = "0.2", default-features = false }
```

## Request and Event

* `Request<'i>` - `seq: UVlq32Backfill` (request id, 0 = no answer expected), `path_kind: PathKind<'i>`, `kind:
  RequestKind<'i>`. `Request::set_seq`/`peek_seq` fill in or read `seq` in an already-serialized buffer without
  touching the rest, since a client picks the id only right before sending.
* `PathKind<'i>` - how a resource is addressed, one of three ways: `Absolute { path }` (a plain index path,
  smallest on the wire), `GlobalCompact { gid: CompactVersion, path_from_trait }` (a global
  [trait id](ww_version.md), for traits used often enough to have one assigned), `GlobalFull { gid: FullVersion,
  trait_name, path_from_trait }` (any crates.io trait, by crate name and version - biggest, since it carries the
  name as a string). A server honors a global path wherever the matching trait sits in its API tree (API-20), after
  checking its own version is protocol-compatible (`FullVersion::is_protocol_compatible`) - what makes a global
  path work across API versions, e.g. for firmware update or device info calls.
* `RequestKind<'i>` - the operation: `Call { args }` / `MultiCall` (same method over an array or several methods
  at once), `Read` / `MultiRead`, `Write { data }` / `MultiWrite`, `StreamSideband { sideband }`, `Introspect`
  (send the `ww_self::ApiBundle` of the addressed resource), `StreamOpen { resume, args }` (open a
  [counted or reliable stream](../api/stream_reliability.md), optionally resuming a `StreamCursor`),
  `Observe { cfg }` (how a trait level's properties are sent, see [below](#observing-properties)). Arguments and
  written values travel as `TailBytes<'i>` - already shrink_wrap-serialized by the generated code on either end,
  opaque to this crate.
* `MultiIndex<'i>` / `MultiArgs<'i>` - which resources a multi-request covers (`Range`, `List`, or `Mask32`) and
  whether every one gets the same serialized arguments (`MultiArgs::Same`) or each its own
  (`MultiArgs::Different`).
* `Event<'i>` - `seq: UVlq32` (echoes the request's, 0 for unsolicited stream/property updates), `result:
  Result<EventKind<'i>, Error<'i>>`.
* `EventKind<'i>` - `Value { data }` (a call's or read's return value), `Written`, `StreamData { path, data }`
  (plain stream/property update), `StreamSideband { path, sideband }`, `StreamItem { path, head: StreamHead, data
  }` (one event of a counted/reliable stream, see [below](#counted-and-reliable-streams)), `PropertyUpdate { path,
  t, missed, data }` (observed properties of one trait level, see [below](#observing-properties)).
* `Error<'i>` / `ErrorKind<'i>` - `err_seq` (a per-call-site id for mapping an error back to the generated code
  that raised it) plus a kind: path/argument/property deserialization failures, `Unimplemented` (an
  [optional item](../api/optional_items.md) the device leaves out), `TraitNotFound` (a global path to a trait the
  device doesn't implement in a compatible version), `UserBytes`/`UserStr` (an API's own error, forwarded
  verbatim), stream session errors, and more.

A `Request` is as small as 3 bytes (`seq` ≤ 127, an empty `Absolute` path - a root call with no arguments) once
`set_seq` has filled in the real `seq`; a `GlobalCompact` call adds about 2 bytes for its `CompactVersion` when
every number in it is ≤ 7.

## Building an `Event`/`Error` while writing its payload

Generated server code often doesn't know yet which `EventKind`/`ErrorKind` variant it's building until the
payload is already (partly) written - a call might succeed or fail partway through serializing its answer.
`EventBuilder`, `EventKindBuilder` and `ErrorBuilder` (`builder` module) use `BufWriter::save_state`/
`restore_state` to reserve space for a result flag / discriminant up front and patch it in afterwards, the same
[discriminant-patching trick](../serdes/showcase.md#patching-a-discriminant-after-youve-already-started-writing-its-payload)
used elsewhere in the wire format - the crate's own tests check the result is byte-for-byte identical to building
the value up front and calling `.to_ww_bytes()`.

## Counted and reliable streams

* `StreamHead` - per-event head of an `EventKind::StreamItem`: `pos`/`units` (where the event's units sit in the
  stream, up to `MAX_POS` = 2⁵⁷-1), `lost` (units the device dropped right before it), `t: Option<Second>` (device
  time), `ext: Option<HeadExt>` (rare markers, evolvable). `end()`/`after_previous()`/`lost_units()` are small
  helpers over these positions. See [stream delivery](../api/stream_reliability.md#head-on-the-wire) for how a
  host turns a sequence of heads into "what did I just lose".
* `StreamCursor` - `session` / `pos`: a point in a stream session, kept by the host and passed back in
  `RequestKind::StreamOpen` to resume after a reconnect.
* `StreamOpened<'i>` - the answer to `StreamOpen`: the (possibly new) `session`, `oldest` position the device still
  has, `live_from` (where the live run starts), `resume_from` (where backfill starts, `None` without one), and
  `result` (the stream function's own serialized return value).

## Observing properties

The `observe` module (API-38) is the protocol for sending a trait level's properties at the rates a host asks for,
any subset of them in one event. A level is one trait (the root API or a `ww_impl!` trait), addressed by its path;
its properties are named by their resource index within the level, as in the introspection data. Only properties
declared with `+observe` can be observed.

* `ObserveRequest<'i>` - body of `RequestKind::Observe`: `entries` (an `ObserveEntry { index, mode }` per
  property), `patch` (`true`: properties not listed keep their setting; `false`: they are `Off`) and `budget`
  (bytes per second the level's updates may take on the link, `None` leaves it to the device).
* `ObserveMode` - `Off`, `OnChange { min_us }` (on a change, at most every `min_us`), `Periodic { every_us }`,
  `OnChangeOrBeat { min_us, max_us }` (on a change, and at least every `max_us`). Periods are in microseconds.
  Evolvable: a mode from a newer host reads as `Unknown(_)` and is refused.
* `ObserveGranted<'i>` - the answer, in an `EventKind::Value`: every property observed now with its mode as
  clamped by the device, the `refused` entries with an `ObserveRefusal` (`NoSuchIndex`, `NotObservable`,
  `ModeNotSupported`, `Duplicate`), and the granted `budget`.
* `EventKind::PropertyUpdate { path, t, missed, data }` - one level's update: device time `t`, `missed` (sends
  that were due but didn't fit the budget since the previous update) and `data`, one slot per resource index with
  the properties sent this time present. `ser_property_update_event` writes it with the data in place. Without
  `t` and `missed`, the event costs 4 bytes plus its data for a level one deep.

Old hosts never send `Observe`, so they never receive a `PropertyUpdate`. A device without observe support answers
`Observe` with `OperationNotSupported`.

## Credit-based flow control for sinks (`credit` module)

`SinkCredit` is the sans-IO device-side state machine behind a `#[credit] sink!(..)` ([credit sinks](../api/streams.md#credit-based-flow-control-for-sinks)):
generated code feeds it every write's byte position and length via `on_write` (returning a `CreditVerdict`:
`Accept`, `Duplicate`, or `Gap` with an optional `Resend` to send), and user code only sets its capacity and calls
`free(len)` as it consumes accepted bytes. `take_credit()` returns a `StreamSideband::Credit` to send once enough
has been freed (or immediately after `open()`); `query(sent)` answers a host's `CreditQuery`. `SinkCredit::new`
takes a plain buffer capacity; `sized_for`/`try_sized_for` additionally take the sink's largest item
(`<SINK>_MAX_ITEM_BYTES` from the generated code) and panic (or error, for `try_sized_for`) if the buffer couldn't
even hold one.

## Worst-case message sizes (`max_len` module)

`call_request`/`call_request_compact`/`value_event` compute the upper bound in bytes of a `Request`/`Event` for a
given resource path and argument/return `MaxSize` (SW-19) - `None` when something in them is unbounded. This is
what backs the generated `<METHOD>_REQUEST_MAX_LEN`/`<METHOD>_RESPONSE_MAX_LEN` consts (API-18), used to size
fixed buffers ahead of time.

## On the wire

`PathKind`, `RequestKind` and `EventKind` are all `final_structure` with a `ww_repr = nib` discriminant - one
nibble to say what kind of request or event this is, no reverse-length for the enum itself since it's always the
outermost or last-written thing. `StreamSideband` and `ErrorKind` similarly pick discriminant widths (`nib`,
`u8`) sized to how many variants they have and how often each is used. `FULL_VERSION`/`COMPACT_VERSION` (the
protocol's own version, built with `wire_weaver::full_version!()` the same way every [`#[ww_trait]`](ww_version.md)
is) are exchanged and checked during link setup with a remote device, independently of any user API's own
version.
