# Changelog

## Unreleased

### 🚀 Features

- `Hub::status() -> HubStatus` (who started the hub, idle exit, time until exit, time left of `keep`, connections) and `DeviceEntry` fields `location`, `label`, `api_hash` (all `Option`, appended with `#[default = None]`), for `ww hub status` and `ww list` through the hub (TOOL-17). Wire-compatible additions.
- New API crate (SRV-28): the ww hub's API. `Hub` (`info`, `handover` frozen; `shutdown`, `keep`) and `Registry`
  (`devices`, `clients`, `take`, `events`) with `HubInfo`, `DeviceEntry`, `DeviceState`, `Holder`, `ClientEntry`,
  `Event`, `HandoverError`, `TakeError`.
