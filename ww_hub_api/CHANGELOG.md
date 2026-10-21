# Changelog

## Unreleased

### 🚀 Features

- `Claims::lend(key, purpose, ttl_s) -> Result<u32, ClaimError>` (SRV-32): a root claim that has the hub close the device for a program outside it until released; `ClaimEntry::lent` (`#[default = false]`), `Event::Lent { key, id }` and `Event::Returned { key }`. Device-wide exclusive access is a claim on the root path, documented on `Claims`. Wire-compatible additions.
- `Hub::status() -> HubStatus` (who started the hub, idle exit, time until exit, time left of `keep`, connections) and `DeviceEntry` fields `location`, `label`, `api_hash` (all `Option`, appended with `#[default = None]`), for `ww hub status` and `ww list` through the hub (TOOL-17). Wire-compatible additions.
- New API crate (SRV-28): the ww hub's API. `Hub` (`info`, `handover` frozen; `shutdown`, `keep`) and `Registry`
  (`devices`, `clients`, `take`, `events`) with `HubInfo`, `DeviceEntry`, `DeviceState`, `Holder`, `ClientEntry`,
  `Event`, `HandoverError`, `TakeError`.
- `Claims` (SRV-34) at `Hub::claims`: `claim(key, path, purpose, ttl_s) -> Result<u32, ClaimError>`, `release(id)`,
  `claims(key)`, with `ClaimEntry`, `ClaimKind`, `ClaimError` (`UnknownDevice`, `ApiUnknown`, `BadPath`,
  `NotClaimable`, `InUse`, `Overlap`), `ReleaseError`; `Event::Claimed` and `Event::Released`.
