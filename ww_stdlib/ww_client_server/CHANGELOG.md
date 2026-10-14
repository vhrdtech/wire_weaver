## Unreleased

### ⚠️ Breaking

Wire-incompatible with 0.1.0.

* `Request::seq` is `UVlq32Backfill` and `Event::seq` is `UVlq32` (were `u16`): seq numbers up to 127 take 1 byte
  instead of 2, and up to `u32::MAX` are possible. `Request::set_seq()` takes a `u32` and returns the slice to send,
  without the unused part of the 5-byte placeholder; send that slice instead of the whole buffer.
  `EventBuilder::new()`, `util::ser_ok_event()`, `ser_err_event()` and `ser_unit_return_event()` take a `u32` seq.
  Unused `Seq` removed.
* `RequestKind`: `MultiCall`, `MultiRead`, `MultiWrite` added (`MultiIndex`, `MultiArgs`); `Subscribe`,
  `Unsubscribe` and `ChangeRate` removed; `StreamSideband` takes a `StreamSideband`.
* `StreamSidebandCommand` and `StreamSidebandEvent` merged into `StreamSideband`, `ShaperConfig` removed.
* `EventKind`: `ReturnValue` and `ReadValue` merged into `Value`; `Subscribed`, `Unsubscribed`, `RateChanged` and
  `Introspect` removed; paths are `RefVec<UNib32>`.
* Payloads use `TailBytes` instead of `RefVec<u8>`, so their length is not encoded twice.
* `Error` and `ErrorKind` take a lifetime; `err_seq` is `UNib32`; `OperationNotImplemented` renamed to
  `Unimplemented`; `UserBytes` and `UserStr` added.

### 🚀 Features

* `max_len::{call_request, call_request_compact, value_event}`: `const fn`s giving the longest request that calls a
  method and the longest reply or stream event, from the arguments' or value's `MaxSize` (SW-19, API-18).
* Credit-based flow control for `#[credit]` sinks (API-28), counted in elements (sink values): `StreamSideband::Credit { have, grant }`,
  `CreditQuery { sent }` and `Resend { have, grant }` appended (existing variants keep their encoding),
  `CreditWrite { pos, data }` (data of a write to a credit sink), and `credit::SinkCredit`, the sans-IO device side
  state that generated server code drives, `SinkCredit::slots_for()` to size a receive queue. See
  `docs/api/streams.md`.
* `Request::peek_seq()` reads the seq of a serialized request without deserializing the rest.
* `EventBuilder`, `EventKindBuilder` and `ErrorBuilder`.
* `MultiIndex::iter()`.
* `PathKind::global()`.
* `COMPACT_VERSION`, using global id `ww_global::WW_CLIENT_SERVER`.

## [0.1.0] - 2026-01-07

### 🚀 Features

* Request
* RequestKind
* PathKind
* Event
* EventKind
* StreamSidebandCommand
* StreamSidebandEvent
* Error