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

## Animations in phase (STD-50)

A device with a shared clock ([`ww_timebase::Clock`](ww_timebase.md#host-sync-std-49), steered by a host or locked
to a sync line) aligns every periodic animation to period boundaries of shared time, which is UTC: a breath or blink
of period `P` shows `t mod P` at shared time `t`, the way WSPR transmissions start on even minutes. Devices showing
the same animation are in phase without an epoch passed around, even when they hang on different PCs (to the
precision of those PCs' NTP, plenty for LEDs). A device whose clock runs free starts an animation when its mode
changes, as before. This is a rule for devices, not an API item: nothing to call, and a device without a shared
clock implements nothing more. Firmware on embedded_bedrock gets it from `bedrock_led` with `bedrock_sync`.

## Evolution

`Mode` and `Brightness` are `Unsized` (the default): a later version may add a variant (an in-between brightness
level, a new mode) without breaking a host built against an older `ww_indication`, which reads it as `Unknown(u32)`
(SW-47).

`tests/indication` has two mock servers: one implementing only `mode`, one with every optional item.
