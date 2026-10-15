## Unreleased

Not published yet, so no version bump.

### ⚠️ Breaking

- The crate is renamed from `ww_ll` to `ww_memory`: depend on `ww_memory` and use `ww_memory::{Memory8, Memory32}`.
- One word type per trait: `Memory32::read` returns `u32` (was `u8`), like its writes, `read_many` and watches; an
  address counts words.
- `Error` is `MemoryError<'i>`: `#[common] Common(CommonError<'i>)` (STD-16), then `WrongAddress` and
  `OutOfWatchSlots`; `WatchNotSupported` is gone (see below). `ww_repr = unib32`, `display`, no longer
  `self_describing` or `sized`; methods list their variants with `#[errors(..)]`. Hosts see `MemoryErrorOwned`.
- `watch`, `watching` and `changed` are the `#[optional(watches)]` group instead of documented as optional: a device
  without watches writes `omit = ["<path>.watches"]` and has no handlers for them. `watching` returns the addresses
  without a `Result`.

### 🚀 Features

- `std` (default) and `defmt` features; `Access` and `MemoryError` have owned variants with `std`.
- Rustdoc for every item and a docs page, `docs/std_library/ww_memory.md`.
