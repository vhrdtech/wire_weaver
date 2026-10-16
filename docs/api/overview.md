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
#[ww_api_root]
trait MyDevice {
    /// Methods: any number of arguments, any return type, Result for errors.
    fn set_brightness(value: f32);
    fn temperature() -> f32;
    fn read() -> Result<u16, SensorError<'i>>;

    /// Properties: values the host reads, writes or observes.
    property!(rw speed: f32);
    property!(ro+observe current: f32);

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
