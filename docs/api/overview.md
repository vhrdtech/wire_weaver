---
level: basic
---

# What an API can have

Blinky's API in [A device on your PC](../basic/first_device.md) has only methods. A trait can hold more: values a
host reads and writes, data that streams out of the device or into it, and groups of items. This page lists them
all, with the chapter that teaches each. From one trait, `ww_codegen!` generates the device's server and the host's
client for every item.

```rust
use ww_si::{Fixed, Point, Range, origins::Celsius, prefix::{Centi, Milli}};
use ww_si::units::{Ampere, Kelvin, Meter, Per, Ratio, Second};

#[ww_api_root]
trait MyDevice {
    /// Methods: any number of arguments, any return type, Result for errors.
    #[effect(actuate)]
    fn set_brightness(value: Fixed<Ratio, Centi, u8, Range<0, 100>>);
    #[effect(read)]
    fn temperature() -> Point<Celsius, Fixed<Kelvin, Centi, i16>>;
    #[effect(read)]
    fn read() -> Result<u16, SensorError>;

    /// Properties: values the host reads, writes or observes.
    #[effect(actuate)]
    property!(rw speed: Fixed<Per<Meter, Second>, Milli, i32>);
    property!(ro+observe current: Fixed<Ampere, Milli, i32>);

    /// Streams out of the device, sinks into it.
    stream!(button: ButtonEvent);
    #[effect(outbound)]
    sink!(uart_tx: [u8]);

    /// Traits: groups of items, here an array of GPIO banks from the standard library.
    ww_impl!(gpio[]: ww_gpio::Bank);
}
```

| Item | Page |
|---|---|
| `fn` | [Methods](../basic/methods.md), and [in depth](methods.md): async or blocking servers, deferred answers, the client |
| `property!`, `const` | [Properties](../basic/properties.md): `get_set` handlers or a plain field, reads, writes, consts |
| `stream!`, `sink!` | [Streams](streams.md): plain, counted and reliable streams, sinks |
| `ww_impl!` | [Traits](traits.md): grouping, reusable traits from crates, attachments |
| `Result<T, E>` | [Errors](errors.md): error enums, common errors, `CallError` on the client |

Everything else builds on these: items can be [arrays](arrays.md) (`gpio[]`), [optional](optional_items.md) for
devices that lack them, [observed](observing.md) without polling, and many can be answered in one
[multi request](multi_requests.md). The [standard library](../std_library/overview.md) has ready-made traits for
GPIO, I2C, UART, firmware updates, logs and more.

The API and its types can evolve: newer hosts work with older devices and the other way round, as long as the
[evolution rules](../evolution/rules.md) are followed.

## Numbers are typed

Almost every number in an API (an argument, a return value, a property, a stream item, a field of a data type) is
typed, not a bare `u32` or `f32`: a physical quantity has its unit in its type, an
[`exact_units::Fixed`](../std_library/exact_units.md) (re-exported by `ww_si`); a count is a counted unit; an
identifier is a newtype; a position or sequence number is `Index`/`IndexFrom1`/`Seq`. `timeout_ms: u16` becomes
`timeout: Millis<u16>`, the same two bytes on the wire and the same code on the device, and `ww`, Python and the
dynamic client show `100 ms`, take `q!(0.1 s)` and refuse what doesn't fit. A bare integer or float stays only with
a reason: a raw protocol field or a truly dimensionless scale.

| Instead of                                | Write                                                   |
|-------------------------------------------|---------------------------------------------------------|
| `timeout_ms: u16`, `uptime_s: u32`        | `timeout: Millis<u16>`, `uptime: Fixed<Second, One, u32>` |
| `buffer_kib: u16`                         | `buffer: Fixed<Byte, Kibi, u16>`                        |
| `battery: u8` (percent)                   | `battery: Fixed<Ratio, Centi, u8, Range<0, 100>>`       |
| `temp: i16` (°C × 100)                    | `temp: Point<Celsius, Fixed<Kelvin, Centi, i16>>`       |
| `item_count: u32`                         | `items: Fixed<Item, One, u32>`                          |
| `scale_id: u8`                            | `scale: ScaleId` (`pub struct ScaleId(u8);`)            |
| `channel: u8` (1 to 4)                    | `channel: IndexFrom1<Item, u8, Range<1, 4>>`            |
| `frame_seq: u16`                          | `frame: Seq<Frame, u16>`                                |

Choose the number type as the smallest that holds the largest value at the device's real resolution (integers unless
the value has no fixed resolution), and give bounds whenever the device can't take every value: they are checked on
every read from the wire. A value whose resolution each device picks is a [`ww_si::Quantity`](../std_library/ww_si.md).
Positions and sequence numbers are on [Indices](../std_library/indices.md), temperatures and other readings against a
zero on [Points](../std_library/points.md). An instant against a named epoch (Unix, GPS) has no type yet (STD-65):
until then it is a `Micros<u64>` with the epoch in its doc comment. `ww api check` points at plain integers whose names
end in a unit (`_ms`, `_us`, `_hz`, `_bytes`, ...).
