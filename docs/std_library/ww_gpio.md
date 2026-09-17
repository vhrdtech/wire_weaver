# ww_gpio

`ww_gpio` is a global WireWeaver trait for controlling GPIO pins remotely: reading and setting output levels,
reading input levels, configuring pin mode / pull / drive strength, and observing edge events as a stream. It is
`no_std` by default and depends only on `wire_weaver` and `ww_si` (for the `Volt` reference-voltage type).

Add it to a device API crate with:

```toml
[dependencies]
ww_gpio = "0.1"
```

## Traits

GPIO pins are grouped into banks (ports), mirroring how most MCUs expose GPIO in hardware. Both levels are traits,
so they compose with the rest of the API using [`ww_impl!`](../api/traits.md) and can be nested as
[arrays](../api/arrays.md).

### `Bank`

One bank (port) of pins sharing the same reference voltage:

```rust
#[ww_trait]
trait Bank {
    ww_impl!(pin[]: Pin);

    fn capabilities() -> BankCapabilities<'i>;

    property!(rw reference_voltage: Volt, Error);

    fn name() -> &'i str;
}
```

* `pin[]` - array of individual pins in the bank, indexed 0-based; see [`Pin`](#pin) below. Indexing can either
  follow the natural range of a bank (e.g. an IO expander) or a list of specific indices (e.g. exposing only `PB1`
  and `PB5` of an MCU port).
* `capabilities()` - static description of what the bank supports: which voltages, modes and custom
  mode/pull/speed strings, see `BankCapabilities` under [Types](#types) below.
* `reference_voltage` - reference voltage used by the whole bank, adjustable if the hardware supports it.
* `name()` - user-friendly bank name (e.g. `"PA"`).

### `Pin`

One pin of a bank. Commonly used operations (`set_output_level`, `toggle`, `output_level`, `input_level`) are
defined first, to get more compact resource paths since low path indices take fewer bits on the wire (see
[traits](../api/traits.md)):

```rust
#[ww_trait]
pub trait Pin {
    fn set_output_level(level: Level);
    fn toggle();
    fn output_level() -> Level;
    fn input_level() -> Level;

    stream!(event: IoPinEvent);

    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>;
    fn mode() -> Mode;

    property!(rw pull: Pull, Error);
    property!(rw speed: Speed, Error);

    fn configure_events(enabled: IoPinEnabledEvents<'i>) -> Result<(), Error>;
}
```

* `set_output_level` / `toggle` / `output_level` - control and read back the pin's output register. If the pin is
  currently an input, these still act on the output register without changing pin mode.
* `input_level` - reads the input register. If the pin is configured as output and its input buffer is disabled,
  the current output level should be returned instead.
* `event` - a stream of `IoPinEvent` (rising/falling edge), enabled per-source through `configure_events`.
* `set_mode` / `mode` - switch between push-pull, open-drain, input, high-z, analog or a custom mode, optionally
  setting the initial output level before the switch. Can fail with `Error::UnsupportedMode`.
* `pull` - pull resistor configuration, can fail with `Error::UnsupportedPull`.
* `speed` - drive strength/slew rate, can fail with `Error::UnsupportedSpeed`.
* `configure_events` - enables/disables which edges (and any custom sources) feed the `event` stream.

## Types

* `Level` - `Low` / `High`, `sized`, 1 bit on the wire.
* `Mode` - `PushPullOutput`, `OpenDrainOutput`, `Input`, `HighZ` (like `Input` but may disable the input buffer to
  save power), `Analog`, `Custom(u8)`; `sized`, 1 nibble.
* `Pull` - `None`, `Up`, `Down`, `Custom(u8)`; `sized`, 2 bits.
* `Speed` - `Slow`, `Medium`, `Fast`, `VeryFast`, `Custom(u8)`; `sized`, 1 nibble.
* `IoPinEvent` - `RisingEdge`, `FallingEdge`, `Custom(u8)`; `sized`, 2 bits.
* `Error` - `UnsupportedMode`, `UnsupportedPull`, `UnsupportedSpeed`, `UnsupportedEventType`,
  `UnsupportedReferenceVoltage`, `DifferentModes`, `NotImplemented`, `CustomU8(u8)`, `CustomU32(u32)`; unsized
  (`unib32` discriminant), evolvable.
* `IoPinEnabledEvents<'i>` - which sources feed the `event` stream: `rising: bool`, `falling: bool`, and a
  `custom: RefVec<'i, u8>` list of custom source indices.
* `BankCapabilities<'i>` - what a bank supports: `voltage: RefVec<'i, Volt>`, `push_pull` / `open_drain` / `input` /
  `individually_configurable_pins: bool`, and `custom_mode` / `custom_pull` / `custom_speed: RefVec<'i, &'i str>`
  describing any `Custom(u8)` variants.

`Level` and `Mode` also carry small helpers: `Level::is_high()` / `is_low()`, `Mode::is_driving()` (true for
`PushPullOutput` and `OpenDrainOutput`).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants and `defmt::Format` are
available by enabling the crate's `std` / `defmt` features.

## Composing into a device API

A device exposes one or more banks by implementing `Bank` as an array on its API root, see the `examples/all_gpio_api`
crate:

```rust
#[ww_api_root]
pub trait AllGpioApi {
    ww_impl!(port[]: ww_gpio::Bank);
}
```

Because `Bank` itself contains `pin[]: Pin`, resources end up two levels deep and addressed by a `[bank_index,
pin_index]` pair, e.g. `port(0).pin(5)`.

## Client usage (`examples/all_gpio`)

The generated client wraps `wire_weaver_client::Commander`; see `examples/all_gpio/src/lib.rs` for the small
`AllGpio` wrapper type. A few usage patterns from `examples/all_gpio/examples`:

Enumerate banks and pins, and read a bank's capabilities (`info.rs`):

```rust
let device = AllGpio::new().connect().await?;

let available_ports = device.port_valid_indices().read().await?;
for port_idx in available_ports.iter() {
    let name = device.port(port_idx).name().call().await?;
    let available_pins = device.port(port_idx).pin_valid_indices().read().await?;
    println!("Port {port_idx}: name: '{name}', pins: {available_pins:?}");
}

let capabilities = device.port(0).capabilities().call().await?;
```

Read every pin's mode sequentially (`mode.rs`) - simple, but one round trip per call:

```rust
for port in ports.iter() {
    for pin in 0..=15 {
        let mode = device.port(port).pin(pin).mode().call().await?;
    }
}
```

The same, but firing all calls concurrently with `futures::future::join_all` (`mode_parallel.rs`) - independent
requests get assembled into as few USB packets as possible by the client, giving a large speedup (940ms -> 55ms on
USB Full Speed in this example, 361ms -> 38ms on High Speed) without changing anything server-side:

```rust
let modes = (0..=15)
    .map(|pin| device.port(port).pin(pin).mode().call())
    .collect::<Vec<_>>();
let modes = join_all(modes).await;
```

`mode_mega_parallel.rs` takes this further, flattening the bank/pin loop into a single `join_all` over every pin of
every bank at once, for the best-case latency (17ms on USB High Speed in this example).

## MCU server implementation (`examples_mcu/nucleo_h743zi2/src/bin/all_gpio.rs`)

On the device side, `ww_codegen!` generates the dispatch code and leaves the actual register access to hand-written
methods on the server state:

```rust
struct ServerState {
    bank: [Gpio; 11],
}

mod server_impl {
    wire_weaver::ww_codegen!(
        all_gpio_api :: AllGpioApi for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
    );
}
```

Each resource path becomes an async method named after the flattened path, e.g. `port_pin_set_output_level`,
`get_port_pin_pull` / `set_port_pin_pull` (property get/set), or `sideband_port_pin_event` (stream enable/disable).
The `[bank_index, pin_index]` addressing is passed in as `index: [UNib32; 2]`:

```rust
async fn port_pin_set_output_level(
    &mut self,
    _msg_tx: &mut impl MessageSink,
    index: [UNib32; 2],
    level: Level,
) -> RpcResult<()> {
    let bank_idx = index[0].0 as usize;
    let pin_idx = index[1].0 as usize;
    let odr = self.bank[bank_idx].odr();
    let level = if level == Level::High { Odr::HIGH } else { Odr::LOW };
    odr.modify(|o| o.set_odr(pin_idx, level));
    Ready(())
}
```

Which indices are valid is reported through `wire_weaver::ValidIndices`, driving what `port_valid_indices()` /
`pin_valid_indices()` return on the client:

```rust
fn valid_indices_root_port(&mut self) -> ValidIndices<'_> {
    ValidIndices::range_u32(0..self.bank.len() as u32)
}

fn valid_indices_root_port_pin(&mut self, index: [UNib32; 1]) -> ValidIndices<'_> {
    ValidIndices::range_u32(0..16)
}
```

In this example, `capabilities()` and `reference_voltage` are hardcoded (3.3V, push-pull/open-drain/input all
supported, no custom modes), `set_mode`/`mode` and `pull`/`speed` are translated to/from the STM32 `MODER` /
`OTYPER` / `PUPDR` / `OSPEEDR` registers, and `configure_events`/the `event` stream are left unimplemented
(`Error::UnsupportedEventType`, sideband handler returns `None`) since the example does not wire up EXTI
interrupts.
