# IO trait family

A set of global WireWeaver trait crates for devices that expose IO over the wire: a dev board bridging its pins to
a host, a USB adapter (an FTDI-like dongle, a Bus Pirate-like probe), a motor controller, a sensor node, a lab
instrument. A device picks the traits it implements and puts each one once into its API tree (`ww_impl!`); nothing
here assumes a particular kind of device.

## Crates

| Crate | Traits | What it's for |
|---|---|---|
| [`ww_gpio`](ww_gpio.md) | `Bank`, `Pin` | Logical-pin GPIO: read, drive, reconfigure, watch edges |
| [`ww_uart`](ww_uart.md) | `Uart` | Bridge a serial port; watch both lines of a link without driving them (optional) |
| [`ww_i2c`](ww_i2c.md) | `I2c`, `I2cWatch`, `I2cMonitor` | Master transfers and bus recovery; autonomous polling; bus sniffing |
| [`ww_spi`](ww_spi.md) | `Spi` | Master transfers with CS held across operations; bus sniffing (optional) |
| [`ww_adc`](ww_adc.md) | `Adc`, `AdcStream`, `AdcCapture` | One calibrated reading; continuous streaming; triggered block capture |
| [`ww_pwm`](ww_pwm.md) | `Pwm`, `PwmCapture` | PWM channel outputs; pulse/frequency measurement on capture-capable channels |
| [`ww_onewire`](ww_onewire.md) | `OneWire` | Reset, ROM search, addressed transactions, strong pull-up |
| [`ww_timebase`](ww_timebase.md) | `Clock` | Device time, clock sync (master/slave), time-of-day for a master |
| [`ww_logic_analyzer`](ww_logic_analyzer.md) | `LogicAnalyzer`, `LogicStream` | Timestamped capture of many digital channels, triggered or streamed |
| [`ww_signal_generator`](ww_signal_generator.md) | `PatternGenerator`, `WaveformGenerator` | Digital pattern playback (replays a capture); DAC waveforms |

[`ww_si`](ww_si.md) and [`ww_numeric`](ww_numeric.md) underpin all of them: every physical value on the wire is a
typed `ww_si` quantity, not a raw integer with an implied unit.

What only some devices have is never a method that answers "not supported" on most implementers. A function with
state, configuration and several items of its own is its own trait: `AdcStream` and `AdcCapture` are separate from
`Adc`, `I2cMonitor` and `I2cWatch` from `I2c`, `PwmCapture` from `Pwm`. A thermometer implements `Adc` only; a data
logger adds `AdcStream`; an I2C sniffer may implement only `I2cMonitor` with no master at all. A function that is one
stream is an [optional item](../api/optional_items.md) of the main trait instead (`Uart::monitor`, `Spi::monitor`),
so an array of ports (`ww_impl!(uart[]: Uart)`) carries it without a second array or trait to keep in step, and a
device without the hardware leaves it out (`omit = ["uart.monitor"]`). A host finds out what a device actually has
through introspection (global trait paths, left-out items) and, within a trait, through its `capabilities()` method.

## Shared conventions

**Capabilities are queried, never assumed.** Every core trait has a `capabilities()`-shaped method (`Bank::capabilities`,
`Pin::capabilities`, `I2c::i2c_capabilities`, `Spi::capabilities`, `Adc::capabilities` /
`AdcStream::stream_capabilities` / `AdcCapture::capture_capabilities`, `Pwm::capabilities`, `OneWire::capabilities`,
`Clock::info`, `LogicAnalyzer::capabilities` / `LogicStream::stream_capabilities`, `PatternGenerator::capabilities`,
`WaveformGenerator::capabilities`) describing ranges, rates, formats and which optional behaviors work, so a host
doesn't have to special-case devices by name.

**One configuration, answered with what was set.** A bus or bank is configured in one call with one struct,
not a property per setting: `Uart::configure(UartConfig)`, `I2c::i2c_configure(I2cConfig)`,
`Spi::configure(SpiConfig)`, `Bank::configure(BankConfig)`, `OneWire::configure(OneWireConfig)`. Each answers with the
same struct as actually set (the baud rate or clock the divider settled on, the IO level in effect), so a host sees
the rounding and a port never runs with half a configuration. Stream functions that configure what they watch do the
same (`Uart::monitor` answers its `MonitorConfig`).

**IO level in the configuration.** On adapters with level translators the IO level is a field of that
configuration, `io_level: Option<Volt>` (`None` leaves it as it is, and is the answer of a device without level
control), and the capabilities list what it can be set to in the same three fields everywhere: `io_levels`
(discrete levels), `io_level_min` / `io_level_max` (an adjustable supply). PWM, ADC, logic analyzer and signal
generator channels sit on pins of a GPIO bank or bus and use its level; the logic analyzer sets its own input
threshold per capture.

**Common error variants.** Each crate defines its own plain error enum (`ww_gpio::Error`, `ww_uart::Error`,
`ww_i2c::I2cError`, `ww_spi::Error`, `ww_adc::AdcError`, `ww_pwm::PwmError`, `ww_onewire::OneWireError`,
`ww_timebase::ClockError`, `ww_logic_analyzer::LaError`, `ww_signal_generator::PgError` / `WgError`), but every one
of them carries the same handful of variants for the same situations: `Unsupported` (this device doesn't have the
feature asked for), `InUse` (the pins or peripheral already serve another function), `NoIoLevel` (the level
translators or output buffers have no supply), `Refused` (a safety rule of the device declines the request). Each
method's rustdoc lists under `# Errors` which variants it can return.

**Typed values through `ww_si`.** Voltages, frequencies, baud rates and durations are `ww_si` quantities (`Volt`,
`Hertz`, `Baud`, `Second`), never a bare integer with an implied unit. The device reports in whatever storage and prefix are
convenient for it (a `u32` of µs, an `f32` of volts); the host reads any of them. Timestamps, in method results
and in stream heads alike, are `ww_si::Second`:
time since the device's own clock started, in the prefix and number type the device picks. A fixed-width value
wraps (a `u32` of µs wraps after about 71.6 minutes); a host that needs the full value reads `Clock::now()` and
unwraps around it. Devices with several timed traits (a logic analyzer, an ADC capture, an I2C monitor) share one
clock, so their timestamps line up against each other.

**Arrays of channels.** A bank, bus or generator with several identical resources (`ww_gpio::Bank::pin`,
`ww_pwm::Pwm::channel`, `ww_signal_generator::WaveformGenerator::channel`) exposes them as `ww_impl!(name[]:
Trait)`, a WireWeaver array of sub-traits. Every other method that addresses one of them takes a plain index into
that same array, in the order its crate's `*Info` / `*Capabilities` type lists them — no bitmasks, no names on the
wire.

**Monitors.** Watching a bus without taking part in it is a counted stream function: `Uart::monitor` and
`Spi::monitor`, optional items of the bus trait, and `I2cMonitor::records`, a trait of its own because I2C
monitoring also exists without a master (`I2cMode::MonitorOnly`) and next to the master's own traffic. A monitor
reports one record per transaction/frame; a master that also wants to see its own traffic sets the matching monitor
flag where the trait offers one (e.g. `I2cMode::Master { monitor: true, .. }`).

**Streams are opened with their configuration.** A stream that needs a configuration is a
[stream function](../api/streams.md#opening-a-stream): the host opens it with the configuration as its arguments and
gets back what the device actually set, or an error, and closing the stream stops sampling or monitoring
(`AdcStream::samples`, `LogicStream::data`, `Pin::event`, `PwmCaptureChannel::capture`, `Spi::monitor`,
`I2cMonitor::records`, `Uart::monitor`). Opening it again starts a new session with the new configuration, so
there is no separate configure / start / stop to keep in sync.

```rust
let (actual, mut samples) = adc.adc_stream().samples(config).call().await??;
```

**Counted streams: positions, losses and time in the head.** Every device → host stream of the family with measured
or received data is counted ([stream delivery](../api/stream_reliability.md)): each event's head carries its
position in the stream's unit (a sample set, a byte, a frame, an edge, as the stream's rustdoc says), how many units
the device dropped right before it, and the device time of its first unit. Item types carry no positions or losses,
and only the time that is their own: an `RxChunk` is bytes, line flags and when its last byte arrived, an
`I2cRecord` / `SpiFrame` when the transaction ended, a `SampleBlock` raw samples. A host sees exactly what it missed and where
(`StreamEvent::Gap`), whether the device dropped it (no host, a full buffer) or the link lost it.

**Credit sinks for bulk host → device data.** Where the host sends more than fits one reply — a UART's `tx` bytes,
a `PatternGenerator`'s pattern, a `WaveformChannel`'s arbitrary table — the method is a `#[credit] sink!(..)`
rather than a plain one: the device's free buffer is the credit, so the host can never overrun it, and `Open` /
`Close` mark where one logical write begins and ends.

## Composing a device API

A device picks whichever traits it supports and puts each one into its API tree once, outside any trait array, so
generic tooling can find it by global trait path:

```rust
#[ww_api_root]
pub trait MyBridge {
    ww_impl!(gpio: ww_gpio::Bank);
    ww_impl!(uart: ww_uart::Uart);
    ww_impl!(i2c: ww_i2c::I2c);
    ww_impl!(i2c_monitor: ww_i2c::I2cMonitor);
    ww_impl!(la: ww_logic_analyzer::LogicAnalyzer);
}
```

Channels of different crates that happen to be the same physical pin cross-reference each other by `gpio_pin:
Option<u8>` (an index into that device's `ww_gpio::Bank::pin`), e.g. `ww_logic_analyzer::ChannelInfo::gpio_pin` or
`ww_signal_generator::PgChannelInfo::gpio_pin`, so a tool can show "logic analyzer channel 3 = PA5 = GPIO pin 5"
without guessing from names.

See each crate's own page — linked in the table above — for its traits, types and a worked example.
