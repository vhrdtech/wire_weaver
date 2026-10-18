## Unreleased

### ⚠️ Breaking

- No `NumericValue::U64` on devices: `ww_si` is pulled in with `time` and `u32` only, no longer `u64`. Stream heads
  and observe timestamps are `Second { Micro, U32 }` on a device; every `NumericValue` (and `NumericValueStore`
  payload) used to be 64-bit on every firmware that depended on this crate. Hosts get `U64` through `std`
  (`ww_si/host`, the device time unwrapper). The wire is the same: a device without `U64` reads one as
  `EnumVariantDisabled` and answers with an error. A firmware that relied on getting `U64` through this crate
  enables `ww_si/u64` itself. On the STM32H725 `shared_clock_cycles` bench: 7.7 KB less flash (94 672 to 86 948
  bytes), `NumericValue` 16 to 8 bytes, the 32-sample FOC level store (21 properties) 6048 to 3360 bytes.

- Batched observe (API-39): `observe::ObserveMode::Samples { every_us, max_age_us }` (every sample, or one per
  `every_us`, in batches at most `max_age_us` old; only for `+observe+batched` properties), and the observe slot
  type of such a property, `observe::Batched<'i, T> { One(T), Samples(Series<T>) }` / `BatchedOwned<T>`, with
  hand-written serdes: a `u4` tag, then the value, or the series from the next byte. The samples of a level share
  one time column per update (API-49): `EventKind::PropertyUpdate` gets `sample_times: Option<Series<Second>>`
  (after `missed`), one time per sample of every `Samples` slot; `BatchedOwned::into_samples(t, sample_times)` pairs
  them (`Error::MalformedSeries` when the counts differ). `ser_property_update_event` takes the encoded
  `sample_times`. `BatchedOne` / `BatchedRaw` / `SeriesRaw` write the same bytes from a borrowed value or an encoded
  series (devices). An old device reads `Samples` as an unknown mode and refuses that entry; `PropertyUpdate` is
  wire-incompatible with hosts and devices built before.

- Batched streams (API-41): `RequestKind::StreamOpen` gets `max_age_us: Option<UNib32>` (how old a batch may get on
  the device before it is sent, the host's latency choice; before `args`) and `stream::StreamOpened` gets
  `max_age_us: Option<UNib32>` (what the device granted; before `result`), with `StreamOpened::with_max_age_us`.
  Wire-incompatible: devices and hosts are rebuilt together. New `stream::ser_stream_data_event` (a `StreamData`
  event of a plain stream written in place, as `ser_stream_item_event` does for positioned ones).

- **Breaking:** credit-sink positions wrap on the wire (API-42): `CreditWrite::pos` and the positions in
  `StreamSideband::Credit`, `CreditQuery` and `Resend` are sent modulo 2^28 (`credit::POS_MODULUS`), so a session has
  no size limit (was 4 GiB) and a write's position never takes more than 4 bytes. The wire types are unchanged and a
  session under 256 MiB has the same bytes; a longer one needs both sides updated (an old host fails at 4 GiB, an old
  device stalls after 256 MiB, a new device drops a position of 2^28 or more as `CreditVerdict::Invalid`).
  `SinkCredit` counts in `u64`: `have()`, `freed()` and `limit()` return `u64`, `on_write()` and `query()` take the
  wire position. A buffer is at most `credit::MAX_WINDOW` (64 MiB): `SinkCredit::new` and `sized_for` panic above it
  (a compile error in a `const`), `try_sized_for` returns the new `SinkCreditError` (replaces `BufferTooSmall`,
  which is now its `BufferTooSmall` variant, next to `BufferTooLarge`), `set_capacity` returns
  `Result<(), SinkCreditError>`. New: `credit::wire_pos()`, `credit::extend_pos()`, `SinkCredit::open_at()` (tests
  of long sessions), `CreditVerdict::Invalid`.

- `ErrorKind` gets `Unknown(u32)` (SW-47): a framework error from a newer device decodes instead of failing the reply.
  `MultiIndex` is `no_future_version`: an index form the device doesn't know fails the request.
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

* `observe` module (API-38): `RequestKind::Observe { cfg: ObserveRequest }` asks the device to send a trait
  level's properties per `ObserveMode` (`Off`, `OnChange`, `Periodic`, `OnChangeOrBeat`) within a byte budget,
  answered with `ObserveGranted` (the clamped settings and the `ObserveRefused` entries);
  `EventKind::PropertyUpdate { path, t, missed, data }` carries any subset of a level's properties in one event,
  `ser_property_update_event` writes it with the data in place. Wire format: both variants are appended, old
  hosts never send `Observe` and so never receive a `PropertyUpdate`.
* `stream::StreamStatus` / `StoreStatus` (a read on a counted or reliable stream), `ser_stream_item_event` (a
  `StreamItem` event with its item in place), `StreamOpened::ser_head` (the stream function's result written in place
  after it), `ErrorKind::StreamNoSession` and `StreamStoreFailed` appended (API-31). Streams no longer use sideband
  `Open`, sinks still do until API-34.
* Counted and reliable streams (API-30), wire types in `stream`: `StreamHead` (position, `units`, `lost`, device
  time `t`, `HeadExt`) with a hand-laid encoding of 1 to 8 bytes for positions up to 2⁵⁷ (flags, the position's
  length and its top bit share the first byte, `units` costs nothing when 1), `StreamCursor`, `StreamOpened`; `EventKind::StreamItem { path, head, data }`,
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
* `MultiIndexOwned::iter_u32()`/`len()`/`is_empty()` (`std`, CLT-15), used by the client's generalized multi-request
  API (`wire_weaver_client::client::multi`).
* `PathKind::global()`.
* `COMPACT_VERSION`, using global id `ww_global::WW_CLIENT_SERVER`.

### 🐛 Fixes

* `MultiIndex::iter()` over a `Range` or `Mask32` panicked with `todo!()` (CLT-15), on a device that's host
  input: a `Range` yields `start..end` (none when `start >= end`), a `Mask32` its set bits, lowest first.
* `RequestKind::discriminants()` of `MultiWrite` is `Value`: it is answered with a value per index, like
  `MultiRead`. The docs of `MultiIndex` and `MultiArgs` describe the answer and the format of `MultiArgs::Different`.

* Depend on `ww_si`'s `time` feature (STD-23 gated `Second` behind it), needed for `stream::StreamHead`'s timestamp.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

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