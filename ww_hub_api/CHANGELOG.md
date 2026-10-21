# Changelog

## Unreleased

### 🚀 Features

- `Claims::lend(key, purpose, ttl_s) -> Result<u32, ClaimError>` (SRV-32): a root claim that has the hub close the device for a program outside it until released; `ClaimEntry::lent` (`#[default = false]`), `Event::Lent { key, id }` and `Event::Returned { key }`. Device-wide exclusive access is a claim on the root path, documented on `Claims`. Wire-compatible additions.
- `ClockSync` (SRV-31) at `Hub::sync`: `devices() -> Vec<SyncEntry>` (per open device: `SyncPhase` `Starting` / `Steering` / `Off` / `Paused` / `Unsupported` / `Failed`, the device's `ww_timebase::SyncState` and source, the steerer it follows and one it only notes, whether it follows the hub, rate in ppb and whether it came from the rate cache, shortest round trip, residual from `shared_now` with its check's round trip, error bound, slew left, samples, steers, restarts, last error), `set_enabled(key, on)` per device or for all, `linger` (rw, seconds, `None` forever) and the `stats` stream. Wire-compatible addition (a new item of the root).
- `Hub::status() -> HubStatus` (who started the hub, idle exit, time until exit, time left of `keep`, connections) and `DeviceEntry` fields `location`, `label`, `api_hash` (all `Option`, appended with `#[default = None]`), for `ww hub status` and `ww list` through the hub (TOOL-17). Wire-compatible additions.
- New API crate (SRV-28): the ww hub's API. `Hub` (`info`, `handover` frozen; `shutdown`, `keep`) and `Registry`
  (`devices`, `clients`, `take`, `events`) with `HubInfo`, `DeviceEntry`, `DeviceState`, `Holder`, `ClientEntry`,
  `Event`, `HandoverError`, `TakeError`.
- `Claims` (SRV-34) at `Hub::claims`: `claim(key, path, purpose, ttl_s) -> Result<u32, ClaimError>`, `release(id)`,
  `claims(key)`, with `ClaimEntry`, `ClaimKind`, `ClaimError` (`UnknownDevice`, `ApiUnknown`, `BadPath`,
  `NotClaimable`, `InUse`, `Overlap`), `ReleaseError`; `Event::Claimed` and `Event::Released`.
