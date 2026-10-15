# ww_gpio

`ww_gpio` is a global WireWeaver trait for controlling GPIO pins remotely: reading and setting output levels,
reading input levels (one pin, or several at one instant), configuring pin mode / pull / drive strength, and
observing edge events as a stream. It is
`no_std` by default and depends only on `wire_weaver` and `ww_si` (for the `Volt` IO level).

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

One bank (port) of pins sharing the same IO level:

```rust
#[ww_trait]
trait Bank {
    ww_impl!(pin[]: Pin);

    fn read_inputs(
        pins: MaxLen<RefVec<'i, UNib32>, MAX_PORT_PINS>,
    ) -> Result<MaxLen<RefVec<'i, Level>, MAX_PORT_PINS>, GpioError<'i>>;
    fn write_outputs(levels: MaxLen<RefVec<'i, PinLevel>, MAX_PORT_PINS>) -> Result<(), GpioError<'i>>;

    reserved!(3..=7);

    fn capabilities() -> BankCapabilities<'i>;

    fn configure(config: BankConfig) -> Result<BankConfig, Error>;

    fn name() -> &'i str;
}
```

* `pin[]` - array of individual pins in the bank, indexed 0-based; see [`Pin`](#pin) below. Indexing can either
  follow the natural range of a bank (e.g. an IO expander) or a list of specific indices (e.g. exposing only `PB1`
  and `PB5` of an MCU port).
* `read_inputs` / `write_outputs` - several pins in one call, at one instant where the hardware allows, see
  [Port access](#port-access) below.
* `capabilities()` - static description of what the bank supports: which IO levels, modes and custom
  mode/pull/speed strings, see `BankCapabilities` under [Types](#types) below.
* `configure` - what the pins of the bank share, in a `BankConfig`: the IO level, adjustable if the hardware has
  level translators (`io_level: None` leaves it as it is). Answers with what was actually set, so `configure` with
  `None` reads the level in effect. Same shape as the `configure` of `ww_uart`, `ww_i2c`, `ww_spi`.
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

    #[stream(IoPinEvent, counted)]
    fn event(enabled: IoPinEnabledEvents<'i>) -> Result<(), Error>;

    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>;
    fn mode() -> Mode;

    property!(rw pull: Pull, Error);
    property!(rw speed: Speed, Error);

    fn capabilities() -> PinCapabilities<'i>;
}
```

* `set_output_level` / `toggle` / `output_level` - control and read back the pin's output register. If the pin is
  currently an input, these still act on the output register without changing pin mode.
* `input_level` - reads the input register. If the pin is configured as output and its input buffer is disabled,
  the current output level should be returned instead.
* `event` - a counted [stream function](../api/streams.md#stream-functions) of `IoPinEvent`s (rising/falling edge,
  custom sources): the host opens it with the sources to report (`Common(Unsupported)` for one the pin can't
  generate) and closes it to stop. Its unit is an event: the event head carries the device time of each
  edge and exactly how many edges the device dropped ([stream delivery](../api/stream_reliability.md)).
* `set_mode` / `mode` - switch between push-pull, open-drain, input, high-z, analog or a custom mode, optionally
  setting the initial output level before the switch. Can fail with `Common(Unsupported)`, or `DifferentModes` on a
  bank whose pins share one mode.
* `pull` - pull resistor configuration, can fail with `Common(Unsupported)`.
* `speed` - drive strength/slew rate, can fail with `Common(Unsupported)`.
* `capabilities()` - what this pin supports: modes, pulls, speeds, whether `event` works, and its name, see
  `PinCapabilities` under [Types](#types) below.

### Port access

`Bank::read_inputs` and `Bank::write_outputs` act on several pins of a bank in one call, for what per-pin calls
can't do even when the client packs them into one packet: change or sample them at the same instant. A host bit-banging a parallel bus sets the data lines and the strobe in one write, or reads a data byte
without a pin changing halfway through it.

* `read_inputs(pins)` - the input levels of the listed pins (indices into `pin[]`), answered in the same order. Reads
  the input register like `Pin::input_level`, whatever mode a pin is in.
* `write_outputs(levels)` - a list of `PinLevel { pin, level }`. Writes the output register like
  `Pin::set_output_level`: no pin changes its mode (direction), an input keeps the level for when it becomes an
  output. All or nothing: an index that isn't a pin of the bank, or a pin listed twice, is
  `Common(InvalidArgument)` and nothing is written; `InUse`, `NoPower` and `Refused` as for `Pin::set_mode`.

`BankCapabilities::simultaneous_read` / `simultaneous_write` say whether a call is one register access. If so, every
listed pin is sampled or changes at the same instant (an STM32 port: one `IDR` read, one `BSRR` write). If not (an
IO expander behind I2C, pins spread over several registers), the device goes pin by pin in list order, still in one
request: list the strobe last. A bank without either answers `Common(Unsupported)`.

Pins are addressed by index, not by a bitmask, as everywhere in the [IO family](io_family.md#shared-conventions):
indices work the same for a bank exposing only some pins of a port (`PB1`, `PB5`) and for banks wider than any mask
type. On the wire an index of a pin below 8 is one nibble, a level one bit, so for 8 pins the argument of
`read_inputs` is 5 bytes and its list of levels 2, and the argument of `write_outputs` (one byte per `PinLevel`) 9. At most `MAX_PORT_PINS` (64) pins per
call.

```rust
let bank = device.port(0);
if bank.capabilities().call().await?.simultaneous_write {
    // data lines 0..=3 and the strobe on pin 4, in one register write
    let levels = [(0, Level::High), (1, Level::Low), (2, Level::Low), (3, Level::High), (4, Level::High)]
        .into_iter()
        .map(|(pin, level)| PinLevel { pin: UNib32(pin), level })
        .collect::<Vec<_>>();
    bank.write_outputs(MaxLen::new(levels)?).call().await?;
}
let levels = bank.read_inputs(MaxLen::new(vec![UNib32(5), UNib32(6)])?).call().await?;
```

Timing of many pins beyond one write (a pattern, a waveform) is a [signal generator](ww_signal_generator.md), fast
sampling of many pins a [logic analyzer](ww_logic_analyzer.md).

### Observing a pin level (planned)

A `property!(ro+observe level: Level)` on `Pin`, at one of its reserved indices 4..=6, would let a host watch slow
inputs without opening the `event` stream of every pin. It waits for array index bounds (API-29): `Pin` sits in the
`Bank::pin[]` array, and [observed properties](../api/properties.md#observing-changes) under a resource array aren't
supported yet (STD-31).

## Types

* `Level` - `Low` / `High`, `sized`, 1 bit on the wire.
* `Mode` - `PushPullOutput`, `OpenDrainOutput`, `Input`, `HighZ` (like `Input` but may disable the input buffer to
  save power), `Analog`, `Custom(u8)`; `sized`, 1 nibble.
* `Pull` - `None`, `Up`, `Down`, `Custom(u8)`; `sized`, 2 bits.
* `Speed` - `Slow`, `Medium`, `Fast`, `VeryFast`, `Custom(u8)`; `sized`, 1 nibble.
* `IoPinEvent` - `RisingEdge`, `FallingEdge`, `Custom(u8)`; `sized`, 2 bits.
* `GpioError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)), `DifferentModes`; unsized
  (`unib32` discriminant), evolvable, `Display` from the doc lines. Each method lists the variants it returns
  (`#[errors]`).
* `PinLevel` - `pin: UNib32` (index into `Bank::pin[]`), `level: Level`; one entry of `Bank::write_outputs`,
  `final_structure`.
* `IoPinEnabledEvents<'i>` - which sources the `event` stream reports, its argument: `rising: bool`, `falling: bool`, and a
  `custom: RefVec<'i, u8>` list of custom source indices.
* `PinCapabilities<'i>` - what one pin supports: `modes: MaxLen<RefVec<'i, Mode>, 8>`,
  `pulls: MaxLen<RefVec<'i, Pull>, 4>`, `speeds: MaxLen<RefVec<'i, Speed>, 5>`, `events: bool` (the `event` stream
  works on this pin), `name: MaxLen<&'i str, 16>` (e.g. `"IO4"`, `"PB5"`).
* `BankConfig` - `io_level: Option<Volt>`; the argument and the answer of `Bank::configure`.
* `BankCapabilities<'i>` - what a bank supports: `io_levels: MaxLen<RefVec<'i, Volt>, MAX_IO_LEVELS>`,
  `io_level_min` / `io_level_max: Option<Volt>` (an adjustable supply), `push_pull` / `open_drain` / `input` /
  `individually_configurable_pins: bool`, `custom_mode` / `custom_pull` / `custom_speed: RefVec<'i, &'i str>`
  describing any `Custom(u8)` variants, and `simultaneous_read` / `simultaneous_write: bool` ([port
  access](#port-access), `false` when an older device leaves them out).

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
`get_port_pin_pull` / `set_port_pin_pull` (property get/set), or `port_pin_event` / `close_port_pin_event` (the stream opened with its sources, closed).
The `[bank_index, pin_index]` addressing is passed in as `index: [UNib32; 2]`:

```rust
async fn port_pin_set_output_level(
    &mut self,
    _cx: &mut Context<'_, impl EventOut>,
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

`port_read_inputs` reads `IDR` once and picks the listed pins from it, `port_write_outputs` collects every entry into
one `BSRR` value (set and reset bits) and writes it once, so the board reports `simultaneous_read` and
`simultaneous_write`.

In this example, `capabilities()` and `configure` are hardcoded (3.3V without level control, push-pull/open-drain/input all
supported, no custom modes), `set_mode`/`mode` and `pull`/`speed` are translated to/from the STM32 `MODER` /
`OTYPER` / `PUPDR` / `OSPEEDR` registers, and the `event` stream never opens (`port_pin_event` answers
`Common(Unsupported)`, one `Counted` state serves every pin) since the example does not wire up EXTI
interrupts.
