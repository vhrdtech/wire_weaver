---
level: basic
---

# ww_indication

Device indication control (STD-28): one global mode every indicator (LED, buzzer, ...) follows.

```rust
#[ww_trait]
pub trait Indication {
    property!(rw mode: Mode);
    #[optional]
    property!(rw brightness: Brightness);
    #[optional]
    fn identify(duration: Second) -> Result<(), CommonError<'i>>;
    #[optional]
    const NAMES: RefVec<'i, &'i str>;
    #[optional]
    fn set_mode_with_epoch(mode: Mode, epoch: Second);
}
```

`mode` is the only item a device must implement: `AllOff`, `OnlyEssential` (only what must stay visible: faults,
power) or `Normal`. Everything else is [optional](../api/optional_items.md), each its own item, so a device picks
what its hardware or flash budget supports:

* `brightness` — one `Brightness` setting (`Lowest` .. `Highest`, 5 levels) for every indicator; a device with no
  adjustable brightness leaves it out with `omit = ["indication.brightness"]`.
* `identify(duration)` — blinks every indicator for `duration` (a [`ww_si::Second`](ww_si.md)) so the unit can be
  found on a bench or in a rack, then returns to `mode`. Errors on [`CommonError`](ww_error.md) directly (no
  indication-specific error): `Unsupported` without a way to blink, `Busy` while already identifying.
* `NAMES` — indicator names in the device's own order, a [`const` item](../api/properties.md#const-items) served
  without a handler. Left out by a device with no room to keep them.
* `set_mode_with_epoch(mode, epoch)` — writes `mode` as the property does, with `epoch` as the start of every
  animation's cycle (STD-50): a breath or blink of period `P` shows `(t - epoch) mod P` at shared time `t`, so devices
  given the same mode and epoch breathe and blink in phase. Shared time comes from the device's
  [`ww_timebase::Clock`](ww_timebase.md#host-sync-std-49) (a host steering it, or a sync line), `epoch` is in the
  prefix and storage of its `Clock::now()` and may lie in the past. A device whose clock runs free starts its
  animations now, as writing `mode` does. `ww sync DEV... --mode normal` steers several devices and sets their mode
  with one epoch ([`ww sync`](../cli_reference.md#ww-sync)).

`Mode` and `Brightness` are `Unsized` (the default): a later version may add a variant (an in-between brightness
level, a new mode) without breaking a host built against an older `ww_indication`, which reads it as `Unknown(u32)`
(SW-47).

`tests/indication` has two mock servers: one implementing only `mode`, one with every optional item.
