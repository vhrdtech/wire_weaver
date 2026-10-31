---
level: basic
---

# Getting started with APIs

A WireWeaver API is a Rust trait describing what a device offers: **methods** to call, **properties** to read and
write, **streams** of data from the device, **sinks** of data into it, and nested **traits** grouping them. From that
one trait, `ww_codegen!` generates the device's server (request decoding and dispatch to your handlers, `no_std` and
no allocation) and the host's client (a typed function per item, async, blocking or as a promise for GUIs). Data
crossing the wire is any [ShrinkWrap](../serdes/shrink_wrap.md) type.

```rust
#[ww_api_root]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
}
```

## The project shape

An API lives in its own small crate, shared by the firmware and the host:

| Crate | What it holds | In this repo |
|---|---|---|
| `blinky_api` | the trait and its data types, `no_std` | `examples/blinky_api` |
| firmware | implements the handlers, runs the server | `examples_mcu/rp2/src/bin/usb_blinky.rs` |
| `blinky` | the generated client, used by apps and scripts | `examples/blinky` |

The API crate's name and version identify the API on the wire, see [Folder structure](folder_structure.md). To
start a new project, copy the [wire_weaver_template](https://github.com/vhrdtech/wire_weaver_template) repository.

## A first API, end to end

**1. The API crate** (`examples/blinky_api/src/lib.rs`):

```rust
#![no_std]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait BlinkyApi {
    fn led_on();
    fn led_off();
}
```

`#[ww_api_root]` marks the top-level trait of a device (`#[ww_trait]` the others). It generates nothing by itself:
`ww_codegen!` reads the trait from its source file.

**2. The device** (from `examples_mcu/rp2/src/bin/usb_blinky.rs`) implements one handler per method:

```rust
struct ServerState {
    led: Output<'static>,
}

impl ServerState {
    async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.led.set_high();
        Ready(())
    }

    async fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.led.set_low();
        Ready(())
    }
}

mod server_impl {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for super::ServerState,
        server = true, no_alloc = true, use_async = true,
    );
}
```

`ww api scaffold blinky_api` ([CLI](../cli_reference.md#ww-api)) prints the struct, a stub for every handler and the
`ww_codegen!` call, so you never write handler signatures by hand. The generated server is sans-IO: a transport
(USB, RTT, UDP, WebSocket, see [Transport protocols](../transport/overview.md)) feeds it requests; the example uses
`wire_weaver_usb_embassy`.

**3. The host** (`examples/blinky`) generates a client and calls the device:

```rust
pub struct Blinky {
    cmd: Commander,
}

impl WwClient for Blinky {
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }
    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

mod api_client {
    wire_weaver::ww_codegen!(blinky_api :: BlinkyApi for crate::Blinky, client = "std_client");
}

// examples/blinky/examples/simple.rs
let device = Blinky::new().connect().await?;
device.led_on().call().await?;
device.led_off().call().await?;
```

`.call().await` is the async form; `.blocking_call()` and `.call_promise(..)` do the same without async. With no
host code at all, `ww introspect` and [Python](../python.md) talk to the device from the API it describes itself.

## What an API can have

```rust
use ww_si::{Fixed, Point, Range, origins::Celsius, prefix::{Centi, Milli}};
use ww_si::units::{Ampere, Kelvin, Meter, Per, Ratio, Second};

#[ww_api_root]
trait MyDevice {
    /// Methods: any number of arguments, any return type, Result for errors.
    fn set_brightness(value: Fixed<Ratio, Centi, u8, Range<0, 100>>);
    fn temperature() -> Point<Celsius, Fixed<Kelvin, Centi, i16>>;
    fn read() -> Result<u16, SensorError<'i>>;

    /// Properties: values the host reads, writes or observes.
    property!(rw speed: Fixed<Per<Meter, Second>, Milli, i32>);
    property!(ro+observe current: Fixed<Ampere, Milli, i32>);

    /// Streams out of the device, sinks into it.
    stream!(button: ButtonEvent);
    sink!(uart_tx: [u8]);

    /// Traits: groups of items, here an array of GPIO banks from the standard library.
    ww_impl!(gpio[]: ww_gpio::Bank);
}
```

| Item | Page |
|---|---|
| `fn` | [Methods](methods.md): handlers, async or blocking servers, deferred answers, the client |
| `property!`, `const` | [Properties](properties.md): `get_set` handlers or a plain field, reads, writes, consts |
| `stream!`, `sink!` | [Streams](streams.md): plain, counted and reliable streams, sinks |
| `ww_impl!` | [Traits](traits.md): grouping, reusable traits from crates, attachments |
| `Result<T, E>` | [Errors](errors.md): error enums, common errors, `CallError` on the client |

Everything else builds on these: items can be [arrays](arrays.md) (`gpio[]`), [optional](optional_items.md) for
devices that lack them, [observed](observing.md) without polling, and many can be answered in one
[multi request](multi_requests.md). The [standard library](../std_library/overview.md) has ready-made traits for
GPIO, I2C, UART, firmware updates, logs and more.

The API and its types can evolve: newer hosts work with older devices and the other way round, as long as the
[evolution rules](../evolution/rules.md) are followed.

## Numbers with units

Every quantity in an API (an argument, a return value, a property, a stream item, a field of a data type) has its
unit in its type: an [`exact_units::Fixed`](../std_library/exact_units.md) (re-exported by `ww_si`), never a `u32`
or `f32` with the unit in its name or in a comment. `timeout_ms: u16` becomes `timeout: Millis<u16>`, the same two
bytes on the wire and the same code on the device, and `ww`, Python and the dynamic client show `100 ms`, take
`q!(0.1 s)` and refuse what doesn't fit.

| Instead of                                | Write                                                   |
|-------------------------------------------|---------------------------------------------------------|
| `timeout_ms: u16`, `uptime_s: u32`        | `timeout: Millis<u16>`, `uptime: Fixed<Second, One, u32>` |
| `buffer_kib: u16`                         | `buffer: Fixed<Byte, Kibi, u16>`                        |
| `battery: u8` (percent)                   | `battery: Fixed<Ratio, Centi, u8, Range<0, 100>>`       |
| `temp: i16` (°C × 100)                    | `temp: Point<Celsius, Fixed<Kelvin, Centi, i16>>`       |
| `channel: u8` (1 to 4)                    | `channel: IndexFrom1<Item, u8, Range<1, 4>>`            |
| `frame_seq: u16`                          | `frame: Seq<Frame, u16>`                                |

Choose the number type as the smallest that holds the largest value at the device's real resolution (integers unless
the value has no fixed resolution), and give bounds whenever the device can't take every value: they are checked on
every read from the wire. A value whose resolution each device picks is a [`ww_si::Quantity`](../std_library/ww_si.md).
Positions and sequence numbers are on [Indices](../std_library/indices.md), temperatures and other readings against a
zero on [Points](../std_library/points.md). An instant against a named epoch (Unix, GPS) has no type yet (STD-65):
until then it is a `Micros<u64>` with the epoch in its doc comment. `ww api check` points at plain integers whose names
end in a unit (`_ms`, `_us`, `_hz`, `_bytes`, ...).
