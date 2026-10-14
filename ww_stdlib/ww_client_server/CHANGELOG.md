## Unreleased

### ⚠️ Breaking

Wire-incompatible with 0.1.0.

* `PathKind::GlobalFull` carries `trait_name: &str` after `gid`: a crate can define several traits, its name and
  version alone don't say which one (API-20). `PathKind::global()` takes the trait name after the compact id.
  Only requests with a `GlobalFull` path change on the wire, no released server served them.
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

* Counted and reliable streams (API-30), wire types in `stream`: `StreamHead` (position, `lost`, device time `t`,
  `HeadExt`) with a hand-laid encoding of 1 to 8 bytes for positions up to 2⁵⁸ (flags, the position's length and its
  top 2 bits share the first byte), `StreamCursor`, `StreamOpened`; `EventKind::StreamItem { path, head, data }`,
  `RequestKind::StreamOpen { resume, args }` and `StreamSideband::{Ack, ResendFrom, Query}` with 64-bit positions
  in units, all appended. Depends on `ww_si` for the timestamp.
* `ErrorKind::TraitNotFound`: answer to a global trait path whose trait the device doesn't implement, implements in
  an incompatible version or only under a resource array (API-20).
* `max_len::{call_request, call_request_compact, value_event}`: `const fn`s giving the longest request that calls a
  method and the longest reply or stream event, from the arguments' or value's `MaxSize` (SW-19, API-18).
* Credit-based flow control for `#[credit]` sinks (API-28), counted in bytes of items (the serialized value of each
  write, never split; an empty item takes one position): `StreamSideband::Credit { have, grant }`,
  `CreditQuery { sent }` and `Resend { have, grant }` appended (existing variants keep their encoding),
  `CreditWrite { pos, data }` (data of a write to a credit sink, `pos` is the byte position of its first byte), and
  `credit::SinkCredit`, the sans-IO device side state that generated server code drives.
  `SinkCredit::sized_for(buffer_bytes, max_item_bytes)` (`const`, a compile error in a `const` when the buffer can't
  hold the largest item) and `try_sized_for()` (`BufferTooSmall` error) also cap the re-grant threshold so the
  largest item always fits after one; `SinkCredit::last_item_len()` tells a write handler what to `free()`.
  See `docs/api/streams.md`.
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