## Unreleased

### ⚠️ Breaking

- Reworked to a lightweight API (STD-28): `mode: Mode` (`AllOff` / `OnlyEssential` / `Normal`) is the only item a
  device must implement. `brightness: Brightness` (5 levels, one setting for all indicators), `identify(duration)`
  (blink everything to find the unit, errors on `CommonError`) and `NAMES` are now `#[optional]`.
- Cut `alert_brightness` / `AlertBrightness`, `animation_enable`, `test_mode` / `TestMode`, and
  `HAS_NOT_CONTROLLABLE`: no replacement, this crate no longer distinguishes alert indicators from informational
  ones, and a per-indicator test mode belongs to a device-specific API.
- `Mode` and `Brightness` are `Unsized` (the default): both may grow a variant later (an in-between brightness
  level, a new mode), and a host built against an older `ww_indication` reads it as `Unknown(u32)` (SW-47) instead
  of failing. Not `no_future_version`, unlike `ww_client_server`'s closed sets.

### 🚀 Features

- `Indication::set_mode_with_epoch(mode, epoch: Second)` (STD-50), `#[optional]`: writes the mode with `epoch`
  (shared time, see `ww_timebase` host sync) as the start of every animation's cycle, so devices given the same
  mode and epoch breathe and blink in phase. Servers that don't have a shared clock leave it out with
  `omit = ["indication.set_mode_with_epoch"]`; servers built before this need that line or the handler.

### 🐛 Fixes

- `Indication::identify` was missing its `#[errors(..)]` list (only a hand-written note); it now has
  `#[errors(Unsupported, Busy)]`.
