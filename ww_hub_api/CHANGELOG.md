# Changelog

## Unreleased

### 🚀 Features

- New API crate (SRV-28): the ww hub's API. `Hub` (`info`, `handover` frozen; `shutdown`, `keep`) and `Registry`
  (`devices`, `clients`, `take`, `events`) with `HubInfo`, `DeviceEntry`, `DeviceState`, `Holder`, `ClientEntry`,
  `Event`, `HandoverError`, `TakeError`.
