---
level: advanced
---

# ww_logic_analyzer

Part of the [IO trait family](io_family.md). `ww_logic_analyzer` is a global WireWeaver trait family for timestamped capture of many digital channels on one
common time base: an MCU with a DMA-able GPIO port or a PIO, up to a dedicated analyzer. It is `no_std` by default
and depends on `wire_weaver`, `ww_si` (`Hertz`/`Second`/`Volt`), `ww_gpio` (`Level`) and `ww_i2c` (`I2cAddress`, for
a protocol trigger stage).

Add it to a device API crate with:

```toml
[dependencies]
ww_logic_analyzer = "0.1"
```

It sits next to [`ww_gpio`](ww_gpio.md) rather than inside it because GPIO is one pin, one call, no timing; here
the whole point is timing: every sample of every channel lands on one grid. Protocol decoding (UART, I2C, SPI, CAN
on top of captured edges) stays on the host; a device that decodes a protocol itself implements that protocol's own
monitor (`Uart::monitor`, `I2cMonitor`, `Spi::monitor`) instead.

## Traits

A device with only capture memory implements `LogicAnalyzer`; one that can stream while sampling adds
`LogicStream`. Both follow the same conventions as the rest of the [IO trait family](io_family.md).

### `LogicAnalyzer`

Triggered capture into device RAM, read out after — the same shape as `ww_adc::AdcCapture`: arm with a
configuration, poll the status, read the data out, abort.

```rust
#[ww_trait]
pub trait LogicAnalyzer {
    fn capabilities() -> LaCapabilities<'i>;
    fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, LaError>;
    fn rearm() -> Result<CaptureActual, LaError>;
    fn status() -> CaptureStatus;
    fn read_raw(range: SampleRange) -> Result<RawChunk<'i>, LaError>;
    fn read_transitions(channel: u8, from: u32) -> Result<TransitionsChunk<'i>, LaError>;
    fn abort();
}
```

* `capabilities` - channels, rates, memory, storage and read encodings, and which triggers work
  (`TriggerCapabilities`).
* `capture` - arms a capture, replacing the previous one and its data, and returns what was actually set. With
  `Trigger::Now` it starts at once; otherwise it samples `pre_trigger` samples continuously and keeps them, waits
  for the trigger, then takes the rest.
* `rearm` - arms again with the last configuration, for repeated captures of the same event without resending it.
* `status` - `Idle`, `Armed`, `Triggered { t }`, or `Done { t_first, trigger_at, samples }` once the data is ready
  — a host polls this until `Done`.
* `read_raw` - samples of a sample range as raw words, at most `MAX_READ_LEN` bytes (`Common(WrongState)` before a
  capture is done, `Common(InvalidArgument)` outside it, `Common(Unsupported)` if the device only keeps
  transitions).
* `read_transitions` - edges of one channel from a sample index on, up to `MAX_EDGES` or a full reply;
  `TransitionsChunk::scanned_to` says where the next call starts (same error set, plus `LaError::NoSuchChannel`).
* `abort` - stops an armed or running capture and drops its data.

Reads are in samples, not bytes, and split by encoding, since a byte offset means nothing for transitions (their
size depends on the signal). `read_transitions` is per channel: it is the cheapest shape for the device (one bit
of every raw word, one pass, a fixed index buffer) and lets a host fetch only the channels it shows; a host
fetching every channel sends one call per channel in one multi-call.

A capture's storage (raw words or transitions) is the device's choice (`LaCapabilities::storage`) and independent
of how it is read out: either storage can be read back in either encoding the device lists.

### `LogicStream`

Continuous capture, streamed while the link keeps up:

```rust
#[ww_trait]
pub trait LogicStream {
    #[stream(StreamChunk<'i>, counted)]
    fn data(config: StreamConfig<'i>) -> Result<StreamActual, LaError>;
    fn stream_capabilities() -> StreamCapabilities<'i>;
}
```

* `data` - a counted [stream function](../api/streams.md#stream-functions): the host opens it with channels, rate
  and encoding and gets back what was actually set (or an `LaError`), then receives raw chunks or per-channel
  transition chunks as they fill. Closing the stream stops sampling, opening it again restarts it with the new
  configuration.
* `stream_capabilities` - the highest raw rate per channel count the link carries, and the highest rate with
  `Transitions` (bounded by the edge rate fitting the link, not the sample rate).

The stream's unit is a **chunk**: the event head says exactly how many chunks the device dropped and where
([stream delivery](../api/stream_reliability.md)), and each chunk's own sample range (`RawChunk::first`,
`TransitionsChunk::from`) tells which samples they covered. The head's `t` is the device time of a chunk's first
sample.

## Encodings

Both traits, and `ww_signal_generator`'s pattern sink, share two encodings so a capture can be replayed without
conversion:

* **`Encoding::Raw`**, [`RawChunk`] - every sample, a word of `RawLayout::width` bits (1, 2, 4, 8, 16 or 32, the
  smallest the device gives for the channel count). Bit `i` of a word is the `i`-th channel of the configuration's
  channel list; sub-byte words are packed starting at bit 0 of a byte, multi-byte words are little-endian — what a
  DMA from a GPIO port produces with no CPU work.
* **`Encoding::Transitions`**, [`TransitionsChunk`] - per channel, its level at the start of the range and the
  sample indices where it toggles, as a `Series<u32>`. A steady clock costs a few bytes per chunk, a quiet
  channel almost nothing (its start level and an empty list), so the edge rate limits a stream, not the sample
  rate: a busy signal sampled for clean edge timing costs the same at a high rate as at a low one.

Sample indices are `u32`, counted from a capture's first sample or from the start of a stream session (where they
wrap and a host unwraps them, as with any fixed-width timestamp in the family).

## Triggers

```rust
pub enum Trigger<'i> {
    Now,
    Stages(MaxLen<RefVec<'i, TriggerStage<'i>>, MAX_STAGES>),
    External { edge: Edge },
}
```

`Trigger::Stages` is met stage after stage, each one after the one before; the capture triggers when the last one
is. One `TriggerStage::Pattern` combines level, edge and pattern conditions on several channels at once
(`ChannelCondition { channel, when: Condition }`, where `Condition` is `Low` / `High` / `Rising` / `Falling` /
`AnyEdge`) — a level trigger is one condition `High`, an edge trigger one `Rising`. `TriggerCapabilities` says how
many stages and conditions a device supports; zero stages means only `Now` and maybe `External` work.

Three protocol stages are matched on the decoded stream next to the capture, decoding only as much as the match
needs, and combine with patterns and with each other like any other stage: `TriggerStage::UartByte` (a UART frame
matching a value under a mask, met at the stop bit), `TriggerStage::I2cAddress` (an address byte matching, of a
read/write/either, met at the ACK bit), `TriggerStage::SpiFirstByte` (the first byte after CS goes low, met at its
last bit). `TriggerCapabilities` lists which protocol stages a device supports.

`CaptureConfig::trigger_out` pulses the device's trigger output when the capture triggers, so another analyzer, a
scope, or a `ww_signal_generator::PatternGenerator` on another device can start on the same edge
(`Start::External`); two devices synced through `ww_timebase::Clock::configure_sync` line their captures up on the
shared time base for anything less exact than a sample.

## Types

* `LaCapabilities<'i>` - `channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, MAX_CHANNELS>`, `rates:
  MaxLen<RefVec<'i, RateLimit>, MAX_RATE_LIMITS>` (highest rate per channel count), `memory: u32`, `storage:
  Encoding`, `raw` / `transitions: bool` (which encodings reads give), `trigger: TriggerCapabilities`.
* `ChannelInfo<'i>` - `name`, `gpio_pin: Option<u8>` (the same pin's index in the device's `ww_gpio::Bank`),
  `threshold: Option<VoltRange>` (if the input threshold is adjustable).
* `RateLimit` - `channels: u8`, `max_rate: Hertz`.
* `TriggerCapabilities` - `max_stages: u8`, `max_conditions: u8`, `edges: bool`, `uart_byte` / `i2c_address` /
  `spi_first_byte: bool`, `external: bool`, `trigger_out: bool`.
* `CaptureConfig<'i>` - `channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>`, `rate: Hertz`, `samples: u32` (total,
  `pre_trigger` included), `pre_trigger: u32`, `trigger: Trigger<'i>`, `threshold: Option<Volt>`, `trigger_out:
  bool`.
* `CaptureActual` - `rate: Hertz`, `samples: u32` (the memory may hold fewer than asked at this channel count),
  `layout: RawLayout`.
* `CaptureStatus` - `Idle`, `Armed`, `Triggered { t: Second }`, `Done { t_first: Second, trigger_at: Option<u32>,
  samples: u32 }`; `sized` (`unib32` discriminant).
* `SampleRange` - `first: u32`, `count: u32`.
* `RawLayout` - `width: u8`.
* `RawChunk<'i>` - `first: u32`, `samples: u32`, `data: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>`.
* `TransitionsChunk<'i>` - `channel: u8`, `level: Level`, `from: u32`, `scanned_to: u32`, `edges:
  Series<'i, u32>` (ascending, at most `MAX_EDGES`, at most 610 bytes).
* `StreamChunk<'i>` - `Raw(RawChunk<'i>)` or `Transitions(TransitionsChunk<'i>)`.
* `StreamCapabilities<'i>` - `raw_rates: MaxLen<RefVec<'i, RateLimit>, MAX_RATE_LIMITS>`, `transitions_rate:
  Option<Hertz>`.
* `StreamConfig<'i>` - `channels`, `rate: Hertz`, `encoding: Encoding`, `flush_interval: Option<Second>`,
  `threshold: Option<Volt>`.
* `StreamActual` - `rate: Hertz`, `layout: RawLayout`.
* `Trigger<'i>`, `TriggerStage<'i>`, `ChannelCondition`, `Condition`, `Edge` - see [Triggers](#triggers).
* `LaError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)) (`Busy`: a stream runs while a
  capture is asked for, or the other way round, on devices that share one sampler), `NoSuchChannel`; unsized (`unib32`
  discriminant), evolvable, shared by both traits.

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants and `defmt::Format` are
available by enabling the crate's `std` / `defmt` features.

## Composing into a device API

```rust
#[ww_api_root]
pub trait MyProbe {
    ww_impl!(la: ww_logic_analyzer::LogicAnalyzer);
}
```

`ChannelInfo::gpio_pin` cross-references the same pin's index in the device's own `ww_gpio::Bank::pin`, so a host
or a generic tool can show which logic-analyzer channel is which GPIO pin. A `ww_signal_generator::PatternGenerator`
on the same or another device replays a capture directly: it accepts the same `StreamChunk` encodings this crate
produces, so a host reads a capture out in whichever encoding the generator lists and feeds the chunks straight to
its pattern sink — see [`ww_signal_generator`](ww_signal_generator.md).
