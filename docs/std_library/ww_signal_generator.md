# ww_signal_generator

`ww_signal_generator` is a global WireWeaver trait family for driving outputs: digital patterns on many pins at a
fixed rate, and analog waveforms on DACs. It is `no_std` by default and depends on `wire_weaver`, `ww_si`
(`Hertz`/`Second`/`Volt`), `ww_logic_analyzer` (reuses its chunk encodings and `Edge`) and `ww_adc` (reuses
`SampleFormat` and `Linear` for DAC tables).

Add it to a device API crate with:

```toml
[dependencies]
ww_signal_generator = "0.1"
```

It is the reverse of [`ww_logic_analyzer`](ww_logic_analyzer.md): that crate samples many digital channels on one
clock, this one drives them. A square wave from a timer pin is [`ww_pwm`](ww_pwm.md), not this crate;
`WaveformGenerator` is for actual DACs (an MCU's DAC with a DMA table, a DDS chip, a lab function generator).

## Traits

A device with a pattern generator (DMA-to-GPIO, a PIO program) implements `PatternGenerator`; one with analog
outputs implements `WaveformGenerator`. A device can have both, neither, or only one.

### `PatternGenerator`

Digital channels driven from a pattern at a fixed rate, all switching on the same tick:

```rust
#[ww_trait]
pub trait PatternGenerator {
    fn capabilities() -> PgCapabilities<'i>;
    fn configure(config: PgConfig<'i>) -> Result<PgActual, PgError>;
    #[credit]
    sink!(pattern: StreamChunk<'i>);
    fn start() -> Result<(), PgError>;
    fn stop();
    fn status() -> PgStatus;
}
```

* `capabilities` - channels, rates, pattern memory, which encodings it accepts (`raw` / `transitions`), and which
  start conditions work (`external_start`, `analyzer_start`).
* `configure` - sets channels, rate, repetition (`Repeat::Once` / `Times(n)` / `Forever`), idle levels and the
  start condition, and returns what was actually set; drops a pattern already loaded.
* `pattern` - the pattern data, appended in order; `Open` starts a new pattern, `Close` marks it complete. The
  sink's credit is the device's free pattern memory, so a host can never overrun it.
* `start` - starts output now, or arms it to wait for `PgConfig::start`. Started before the pattern is complete, it
  plays what it has while the sink keeps filling, and stops with `PgStatus::Underrun` if the host falls behind.
* `stop` - stops output; channels go to their idle levels.
* `status` - `Idle`, `Loading { samples }`, `Ready`, `Running { repeat, t_start }`, `Done { t_start }`, `Underrun`.

**Replaying a capture.** The `pattern` sink takes `ww_logic_analyzer::StreamChunk` — `Raw` words or per-channel
`Transitions` — the very item type `LogicAnalyzer::read_raw` / `read_transitions` and `LogicStream` produce. A host
replays a capture by reading it out in an encoding the generator accepts (`PgCapabilities::raw` /
`::transitions`) and feeding the chunks to the sink unchanged, with the capture's rate and channel mapping carried
over into `PgConfig`.

**Start conditions** (`Start`): `Now` (on `start()`), `External { edge }` (the external trigger input), or
`AnalyzerTrigger` (this device's own `LogicAnalyzer` triggering) — a stimulus and its response sharing one time
base. `PgCapabilities::external_start` / `analyzer_start` say which work.

**Pins.** While a pattern plays, its channels belong to the generator: `ww_gpio::Pin::set_mode` /
`set_output_level` on the same pin answer `PgError::InUse`; reading the level still works.

### `WaveformGenerator`

DAC outputs, one sub-trait per channel:

```rust
#[ww_trait]
pub trait WaveformGenerator {
    ww_impl!(channel[]: WaveformChannel);
    fn capabilities() -> WgCapabilities<'i>;
    fn start_together(channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>) -> Result<(), WgError>;
}

#[ww_trait]
pub trait WaveformChannel {
    fn set(waveform: Waveform) -> Result<WaveformActual, WgError>;
    fn stop(level: Volt) -> Result<Volt, WgError>;
    fn state() -> WaveformState;
    #[credit]
    sink!(table: MaxLen<RefVec<'i, u8>, MAX_TABLE_CHUNK>);
}
```

* `capabilities` - the channels and their ranges, rates, resolution and table formats (`WgChannelInfo`).
* `start_together` - starts every already-`set` channel in the list on the same sample clock tick, so their phases
  line up against each other (`WgError::Unsupported` if the channels don't share a clock).
* `WaveformChannel::set` - starts or changes one output's waveform and returns what the DAC actually does (clamped
  levels, the frequency its sample clock divides to).
* `stop` - stops and holds a level, returning the level actually held.
* `state` - `Stopped { level }`, `Held { waveform }` (set, waiting for `start_together`), or `Running { waveform,
  actual }`.
* `table` - raw DAC codes of a `Shape::Arbitrary` waveform, appended in order (`Open` / `Close` bracket one table);
  the credit is the channel's free table memory.

**Shapes** (`Waveform { shape, freq, amplitude, offset, phase }`, amplitude peak-to-peak, phase against the other
channels in `start_together`): `Sine`, `Square { duty }`, `Triangle`, `Sawtooth { rising }`, `Dc` (offset only), and
`Arbitrary { format, once }` — the `table` sink's codes played once per period, or once and held with `once`; fed
faster than it plays, a `once` table is a streamed DAC output. Table codes are raw, converted to volts by
`WgChannelInfo::code_to_volt` (a `Linear`, the same pair `ww_adc` uses the other way round).

## Types

* `PgCapabilities<'i>` - `channels: MaxLen<RefVec<'i, PgChannelInfo<'i>>, MAX_CHANNELS>`, `rates:
  MaxLen<RefVec<'i, RateLimit>, MAX_RATE_LIMITS>`, `memory: u32`, `raw` / `transitions: bool`, `external_start` /
  `analyzer_start: bool`.
* `PgChannelInfo<'i>` - `name`, `gpio_pin: Option<u8>` (the same pin's index in the device's `ww_gpio::Bank`).
* `PgConfig<'i>` - `channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>`, `rate: Hertz`, `repeat: Repeat`, `idle: u32`
  (bit `i` is the idle level of `channels[i]`), `start: Start`.
* `Repeat` - `Once`, `Times(u32)`, `Forever`.
* `Start` - `Now`, `External { edge: Edge }`, `AnalyzerTrigger`.
* `PgActual` - `rate: Hertz`, `width: u8` (the raw word width the generator expects).
* `PgStatus` - `Idle`, `Loading { samples: u32 }`, `Ready`, `Running { repeat: u32, t_start: Second }`, `Done {
  t_start: Second }`, `Underrun`; `sized` (`unib32` discriminant).
* `PgError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)), `NoSuchChannel`; unsized
  (`unib32` discriminant), evolvable, used by `PatternGenerator`.
* `WgCapabilities<'i>` - `channels: MaxLen<RefVec<'i, WgChannelInfo<'i>>, MAX_CHANNELS>`.
* `WgChannelInfo<'i>` - `name`, `min` / `max: Volt`, `max_freq: Hertz`, `max_sample_rate: Hertz`, `bits: u8`,
  `table_len: u32`, `code_to_volt: Linear`, `formats: MaxLen<RefVec<'i, SampleFormat>, MAX_FORMATS>`.
* `Waveform` - `shape: Shape`, `freq: Hertz`, `amplitude: Volt`, `offset: Volt`, `phase: u16` (`0..=65535` =
  `0..360°`).
* `Shape` - `Sine`, `Square { duty: u16 }` (`0..=65535` = `0..100%`), `Triangle`, `Sawtooth { rising: bool }`,
  `Dc`, `Arbitrary { format: SampleFormat, once: bool }`.
* `WaveformActual` - `freq: Hertz`, `amplitude: Volt`, `offset: Volt`.
* `WaveformState` - `Stopped { level: Volt }`, `Held { waveform: Waveform }`, `Running { waveform: Waveform, actual:
  WaveformActual }`; `sized` (`unib32` discriminant).
* `WgError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)), `NoSuchChannel`; unsized
  (`unib32` discriminant), evolvable, shared by `WaveformGenerator` and `WaveformChannel`.

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants and `defmt::Format` are
available by enabling the crate's `std` / `defmt` features.

## Composing into a device API

```rust
#[ww_api_root]
pub trait MyBench {
    ww_impl!(la: ww_logic_analyzer::LogicAnalyzer);
    ww_impl!(pattern: ww_signal_generator::PatternGenerator);
    ww_impl!(dac: ww_signal_generator::WaveformGenerator);
}
```

A device with both `ww_logic_analyzer` and `PatternGenerator` can replay what it just captured: read the capture
out in a shared encoding and feed it straight to the `pattern` sink, see
[`ww_logic_analyzer`](ww_logic_analyzer.md#composing-into-a-device-api).
