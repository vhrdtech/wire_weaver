## Unreleased

### ⚠️ Breaking

Wire-incompatible with 0.1.0.

* `RequestKind`: `MultiCall`, `MultiRead`, `MultiWrite` added (`MultiIndex`, `MultiArgs`); `Subscribe`,
  `Unsubscribe` and `ChangeRate` removed; `StreamSideband` takes a `StreamSideband`.
* `StreamSidebandCommand` and `StreamSidebandEvent` merged into `StreamSideband`, `ShaperConfig` removed.
* `EventKind`: `ReturnValue` and `ReadValue` merged into `Value`; `Subscribed`, `Unsubscribed`, `RateChanged` and
  `Introspect` removed; paths are `RefVec<UNib32>`.
* Payloads use `TailBytes` instead of `RefVec<u8>`, so their length is not encoded twice.
* `Error` and `ErrorKind` take a lifetime; `err_seq` is `UNib32`; `OperationNotImplemented` renamed to
  `Unimplemented`; `UserBytes` and `UserStr` added.

### 🚀 Features

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