# Changelog

## Unreleased

### 📚 Documentation

- A README, crate-level docs, and a comment on the `std` feature.

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.
- Depends on `ww_api` instead of `wire_weaver` (API-68): the crate no longer pulls the runtime in; `std`/`defmt` forward to `ww_api`.
- The API's wire version `w1.0` is declared in `Cargo.toml` (`[package.metadata.wire_weaver]`, EVO-31) instead of `#[ww_api_root(wire = ..)]`; the same wire version, no wire change. Needs a `wire_weaver_derive` with EVO-31.

### 🚀 Features

- Every method and writable property declares its effect (API-58).
- `HubStatus::captures`: whether the hub records (SRV-64), `false` from an older hub.
- `Captures` at `Hub::captures` (SRV-64): `start(key, CaptureOptions) -> CaptureId`, `stop`, `save(id, path) -> SaveInfo`, `discard` and `set_ram_max(id, RamMax)` (only for a client with a part in the capture), `captures(after) -> CapturePage` (paged by `MAX_CAPTURE_PAGE_BYTES`, `CaptureEntry::clients` as connection ids), `watch(id, Pressure) -> WatchId` and `watches[w].records`, a stream of `RecordBatch` (`WatchEntry::Link`, `Record`, `Gap` with `GapCause`); `CaptureError` with `NotAvailable { hub, started_by }` and its `Display` text. A relaying hub may only start and stop, within a quota for all relaying hubs, and never widens a local capture. A capture that ends unsaved drops its records. `CaptureEntry::broken` (a tap panicked) and `CaptureError::Closing` (the hub exits). Every length and count is bounded by a constant (`MAX_KEY_LEN`, `MAX_PATH_LEN`, `MAX_CAPTURES`, `MAX_CAPTURE_CLIENTS`, `MAX_RELAY_CAPTURES`, `MAX_CAPTURE_PAGE_BYTES`, `MAX_WATCHES`, `MAX_BATCH_ENTRIES`, `MAX_BATCH_BYTES`, `MAX_ITEM_LEN`, `MAX_LINK_KEY_LEN`, `RAM_MAX_MIN`/`MAX`/`DEFAULT`). Depends on `ww_si` (`time`, `information`, `counting`).
- `api_error!` for `TakeError`, `ClaimError`, `ReleaseError`, `AdoptError`, `HandoverError` and their `Owned` forms (STD-16): the hub's handlers `?` them into `wire_weaver::Error`.
- `DeviceState::Lent { claim }` (SRV-32): a device lent out with `Claims::lend`, with the claim naming who borrowed it and what for; it was listed as free (or not at all) while lent. Wire-compatible addition: an older client reads it as an unknown state.
- `Adopt` (CLT-44) at `Hub::adopt`: `prepare(snapshot) -> Result<u64, AdoptError>` and `commit(id) -> Result<(), AdoptError>`, a direct holder handing its live link to the hub in two steps; `AdoptError` (`Versions`, `Snapshot`, `Busy`, `UnknownId`, `Open`). Wire-compatible addition (a new item of the root).
- `Claims::lend(key, purpose, ttl_s) -> Result<u32, ClaimError>` (SRV-32): a root claim that has the hub close the device for a program outside it until released; `ClaimEntry::lent` (`#[default = false]`), `Event::Lent { key, id }` and `Event::Returned { key }`. Device-wide exclusive access is a claim on the root path, documented on `Claims`. Wire-compatible additions.
- `ClockSync` (SRV-31) at `Hub::sync`: `devices() -> Vec<SyncEntry>` (per open device: `SyncPhase` `Starting` / `Steering` / `Off` / `Paused` / `Unsupported` / `Failed`, the device's `ww_timebase::SyncState` and source, the steerer it follows and one it only notes, whether it follows the hub, rate in ppb and whether it came from the rate cache, shortest round trip, residual from `shared_now` with its check's round trip, error bound, slew left, samples, steers, restarts, last error), `set_enabled(key, on)` per device or for all, `linger` (rw, seconds, `None` forever) and the `stats` stream. Wire-compatible addition (a new item of the root).
- `Hub::status() -> HubStatus` (who started the hub, idle exit, time until exit, time left of `keep`, connections) and `DeviceEntry` fields `location`, `label`, `api_hash` (all `Option`, appended with `#[default = None]`), for `ww hub status` and `ww list` through the hub (TOOL-17). Wire-compatible additions.
- New API crate (SRV-28): the ww hub's API. `Hub` (`info`, `handover` frozen; `shutdown`, `keep`) and `Registry`
  (`devices`, `clients`, `take`, `events`) with `HubInfo`, `DeviceEntry`, `DeviceState`, `Holder`, `ClientEntry`,
  `Event`, `HandoverError`, `TakeError`.
- `Claims` (SRV-34) at `Hub::claims`: `claim(key, path, purpose, ttl_s) -> Result<u32, ClaimError>`, `release(id)`,
  `claims(key)`, with `ClaimEntry`, `ClaimKind`, `ClaimError` (`UnknownDevice`, `ApiUnknown`, `BadPath`,
  `NotClaimable`, `InUse`, `Overlap`), `ReleaseError`; `Event::Claimed` and `Event::Released`.
