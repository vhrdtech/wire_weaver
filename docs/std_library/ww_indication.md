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

`Mode` and `Brightness` are `Unsized` (the default): a later version may add a variant (an in-between brightness
level, a new mode) without breaking a host built against an older `ww_indication`, which reads it as `Unknown(u32)`
(SW-47).

`tests/indication` has two mock servers: one implementing only `mode`, one with every optional item.
