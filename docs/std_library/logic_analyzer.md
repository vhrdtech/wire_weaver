# Logic analyzer and signal generator (design)

!!! note "Draft for review"
    STD-21 `ww_logic_analyzer` and STD-22 `ww_signal_generator`, the design pass the
    [IO family](io_family.md) asked for. Nothing is built yet; questions for Roman are in [Open questions](#open-questions).
    Everything here follows the family's conventions (errors, streams, capabilities, `ww_si` values); only what is
    new is written out.

## What the two crates are for

`ww_logic_analyzer` samples many digital channels at one fixed rate, with a common time base, from an MCU with a
DMA-able port or a PIO up to a dedicated analyzer. `ww_signal_generator` is the reverse: it drives many digital
channels from a pattern at a fixed rate (`PatternGenerator`), or an analog output from a waveform
(`WaveformGenerator`, a DAC).

They exist next to `ww_gpio` because GPIO is one pin, one call, no timing; here the whole point is timing: every
sample of every channel lands on one grid, and many pins switch on the same tick.

| Crate | Trait | What it is for | Who implements it |
|---|---|---|---|
| `ww_logic_analyzer` | `LogicAnalyzer` | Triggered capture into device RAM, read out after; lossless at any rate the hardware can sample | Every analyzer |
| | `LogicStream` | Continuous capture streamed while the link keeps up | Devices with a fast link or sparse signals |
| `ww_signal_generator` | `PatternGenerator` | Digital channels driven from a pattern, one-shot or repeated; replays an analyzer capture | MCUs with DMA-to-GPIO or a PIO, pattern generators |
| | `WaveformGenerator` | `channel[]` of DAC outputs: sine, square, triangle, sawtooth, DC, arbitrary tables | Devices with a DAC |

Protocol decoding (UART, I2C, SPI, CAN on top of captured edges) stays on the host, where sigrok/PulseView and our
own decoders live. A device that decodes a protocol itself implements that protocol's monitor trait
(`I2cMonitor`, `UartMonitor`, `SpiMonitor`) instead.

## Encodings: `Raw` and `Transitions`

These are the reason the crate exists. Both are used by the analyzer (to read captures and to stream) and by the
pattern generator (to load patterns), so a capture can be replayed without conversion.

### `Raw`: every sample, bit-packed

Samples one after another, each a word of `width` bits (1, 2, 4, 8, 16 or 32, the smallest the device can give for
the channel count, see [`RawLayout`](#types)). Bit `i` of a word is the `i`-th channel of the configuration's channel
list; sub-byte words are packed into a byte starting at bit 0 (sample 0 in bits 0..width), multi-byte words are
little-endian. This is what a DMA from a GPIO port produces with no CPU work (a port is 8, 16 or 32 bits) and what
sigrok calls `unitsize` for whole bytes.

Raw is the right encoding for busy signals (a fast clock sampled near its Nyquist rate), for devices with no CPU
time to spare, and for the pattern generator's DMA. Its size is `samples × width / 8` regardless of what the
signals do.

### `Transitions`: only the edges, per channel

For each channel: its level at the start of the range and the sample indices where it toggles, as a
`DeltaOfDelta<'i, u32>` (SW-31). Every listed index is an edge of that channel (levels are implied: each one
toggles), so no level bits are stored. A channel that keeps a steady rhythm (a clock, a UART at a fixed baud,
PWM at a fixed duty) costs about one bit per edge; a quiet channel costs its start level and an empty list.

Measured with a scratch encoder on this branch (synthetic captures, `DeltaOfDelta` / `Delta` from `shrink_wrap`
as they are on master):

| Capture | Raw | Merged + DoD | Per channel, DoD | Per channel, `Delta` | Per channel, DoD, chunks of 128 edges |
|---|---:|---:|---:|---:|---:|
| SPI 1 MHz at 8 MS/s, 4 ch, 512 bytes | 18944 B | 5233 B | **4584 B** | 14080 B | 5607 B |
| same SPI on 8 channels (4 idle) | 37888 B | 9344 B | **4588 B** | 14084 B | 5607 B |
| UART 115200 at 1 MS/s, 512 bytes | 7292 B | 3149 B | **2803 B** | 3148 B | 3019 B |
| UART 115200 at 24 MS/s, 512 bytes | 175000 B | 4304 B | **3954 B** | 4886 B | 4168 B |
| I2C 400 kHz at 4 MS/s, 2 ch, 320 bytes | 8080 B | 2302 B | **2076 B** | 8196 B | 2733 B |
| PWM 1 kHz 30 % at 1 MS/s, 100 ms | 12500 B | 427 B | **402 B** | 402 B | 416 B |
| 8 ch: 1 kHz clock + slow counter, 100 ms | 100000 B | 232 B | **51 B** | 423 B | 74 B |

"Merged" is the alternative layout: one index list of every sample where any channel changed, plus the new level
word (one bit per channel) for each. Per channel wins in every case and doesn't grow with idle channels, which is
why it is the proposal. `DeltaOfDelta` beats `Delta` wherever a clock is involved (a constant interval is 1 bit);
both lose on PWM with an uneven duty (intervals alternate 300 / 700, every change of the interval costs a 16-bit
bucket), which is still 3 % of raw. The last column is what replies and stream items actually cost: each chunk
restarts the code with its first index at full width.

What it gives: the sample rate stops being the limit, the edge rate is. A UART sampled at 24 MS/s for clean edge
timing costs as much as at 1 MS/s; a mostly idle bus can be streamed at a rate whose raw form no link carries.

**Device side.** Transitions are made from raw samples by the device's CPU: XOR of consecutive words gives the
changed channels, one pass per read. A device that can't spare the CPU lists only `Raw`; one that samples through
edge-capturing hardware (timer input capture, a PIO program that counts) can store transitions directly and list
only `Transitions`, the host converts. `shrink_wrap`'s `DeltaOfDelta` serializes from a slice, so a device collects
up to a chunk's worth of indices (128 × `u32`, 512 bytes of RAM per channel being encoded) and serializes them in
one go; the worst case of 128 indices is 128 × 37 bits = 592 bytes, which fits a reply.

**Indices** are `u32` sample numbers counted from the capture's first sample (`LogicAnalyzer`) or from the start
of the stream (`LogicStream`). At 100 MS/s a `u32` wraps after 43 s; the stream's indices wrap and the host unwraps
them like any fixed-width timestamp (the D6 rule of the family); captures longer than 2³² samples are read in
ranges and each chunk carries its own base (see [`TransitionsChunk`](#types)).

## `LogicAnalyzer`: triggered capture into RAM

Same shape as `AdcCapture`, so a generic tool that knows one knows the other: arm with a configuration, poll the
status, read the data out in pieces, abort.

```rust
#[ww_trait]
pub trait LogicAnalyzer {
    /// Channels, rates, memory, encodings, triggers.
    fn capabilities() -> LaCapabilities<'i>;

    /// Arms a capture (replacing the previous one and its data) and returns what was actually set.
    fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, LaError>;

    /// Arms again with the last configuration (repeated captures of the same event).
    fn rearm() -> Result<CaptureActual, LaError>;

    /// Where the capture stands.
    fn status() -> CaptureStatus;

    /// Samples `range` of a finished capture as raw words, at most `MAX_READ_LEN` bytes; returns how many samples
    /// it holds (a whole number of words, possibly fewer than asked).
    fn read_raw(range: SampleRange) -> Result<RawChunk<'i>, LaError>;

    /// Edges of `channel` from `from` on, up to one chunk; the reply says up to which sample it looked.
    fn read_transitions(channel: u8, from: u32) -> Result<TransitionsChunk<'i>, LaError>;

    /// Stops an armed or running capture and drops its data.
    fn abort();
}
```

Notes against the sketch in [io_family](io_family.md#ww_logic_analyzer-new) (`configure`, `arm`, `status`,
`read(offset, len, encoding)`):

- **`capture(config)` arms**, as in `AdcCapture`; `rearm()` covers the scope-like "single, again, again" without
  sending the configuration each time.
- **Reads are in samples, not bytes**, and split by encoding: a byte offset means nothing for transitions, whose
  size depends on the signal.
- **`read_transitions` is per channel.** It is the cheapest for the device (one bit of every word, one pass, a
  fixed buffer of indices) and lets a host fetch only the channels it shows. A host fetching all channels sends
  one call per channel in one multi-call (CLT-15). Each reply holds edges in `from..scanned_to`; the next call starts
  at `scanned_to`. A device stops at 128 edges or when the reply would overflow, whichever comes first.

**Configuration.**

```rust
pub struct CaptureConfig<'i> {
    /// Channel numbers ([LaCapabilities::channels]), in the bit order of raw words.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    /// Samples per second.
    pub rate: Hertz,
    /// Samples to keep in total, `pre_trigger` included.
    pub samples: u32,
    /// Samples to keep from before the trigger.
    pub pre_trigger: u32,
    pub trigger: Trigger<'i>,
    /// Input threshold where it is adjustable; `None` leaves it.
    pub threshold: Option<Volt>,
    /// Pulse the trigger output when the capture triggers.
    pub trigger_out: bool,
}
```

`CaptureActual` returns the real `rate` and `samples` (the memory may hold fewer at this channel count), and the
`RawLayout` (`width` per sample) raw reads will have.

**Storage.** The device decides how it keeps a capture: as raw words (most MCUs, DMA into a buffer) or as
transitions (edge capture hardware, or a CPU compressing on the fly like Saleae devices do). With transitions in
RAM, `samples` can be far larger than the memory in raw words; the capture ends when `samples` are taken or the
memory fills, and `CaptureStatus::Done` says how many samples it covers. `LaCapabilities::storage` says which, so a
host can offer long captures where they make sense. Either storage is read in either encoding the device lists.

**Status.**

```rust
pub enum CaptureStatus {
    Idle,
    /// Sampling the pre-trigger part, waiting for the trigger.
    Armed,
    /// Triggered at `t`, taking the rest.
    Triggered { t: Second },
    /// Done. Sample 0 was taken at `t_first`; the trigger fired at sample `trigger_at`. `samples` may be fewer than
    /// asked when transition storage filled up.
    Done { t_first: Second, trigger_at: Option<u32>, samples: u32 },
}
```

Times are `ww_si::Second` of the device clock (`ww_timebase::Clock`), as everywhere in the family; the time of
sample `k` is `t_first + k / rate` with the actual rate. `trigger_at` is a sample index, exact to the sample, rather
than a second timestamp.

## Triggers

```rust
pub enum Trigger<'i> {
    /// Right away.
    Now,
    /// When every condition of a stage holds on the same sample, stage after stage; the capture triggers at the
    /// last stage.
    Stages(MaxLen<RefVec<'i, TriggerStage<'i>>, MAX_STAGES>),
    /// On the external trigger input.
    External { edge: Edge },
}

pub struct TriggerStage<'i> {
    pub conditions: MaxLen<RefVec<'i, ChannelCondition>, MAX_CHANNELS>,
}

pub struct ChannelCondition {
    pub channel: u8,
    pub when: Condition,
}

/// `ww_repr = u3`
pub enum Condition { Low, High, Rising, Falling, AnyEdge }

/// `ww_repr = u2`
pub enum Edge { Rising, Falling, Any }
```

This is sigrok's trigger model, which covers the cases the family named: **level** (one condition `High`),
**edge** (`Rising`), **pattern** (several `Low` / `High` in one stage), and an edge qualified by a pattern ("CLK
rising while CS low"). Stages give sequences ("CS falls, then MOSI rises") on hardware that has them;
`LaCapabilities::trigger` says how many stages and which conditions work (an MCU with a pin-change interrupt does
one stage of edges on a few channels; an FPGA analyzer does several stages of anything).

Not in the first version, and appendable as `Trigger` variants without breaking anything: protocol triggers
(UART byte, I2C address match), pulse-width triggers ("a low pulse shorter than 1 µs"), counted triggers ("the 10th
rising edge").

**Trigger output.** `CaptureConfig::trigger_out: bool` drives the device's trigger output when the capture
triggers, so another analyzer, a scope or a `PatternGenerator` on another device starts on the same edge. For
alignment without wires, the timestamps line up through `ww_timebase` (below).

## `LogicStream`: continuous capture

```rust
#[ww_trait]
pub trait LogicStream {
    /// Captured data, in the configured encoding.
    stream!(data: StreamChunk<'i>);

    fn stream_capabilities() -> StreamCapabilities<'i>;

    /// Sets up streaming and returns what was set. Sideband `Open` starts sampling, `Close` stops it.
    fn configure_stream(config: StreamConfig<'i>) -> Result<StreamActual, LaError>;

    fn stream_state() -> StreamState<'i>;
}

pub struct StreamConfig<'i> {
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    pub rate: Hertz,
    pub encoding: Encoding,
    /// Send a chunk at least this often even if it isn't full; `None` lets the device choose.
    pub flush_interval: Option<Second>,
    pub threshold: Option<Volt>,
}

pub enum StreamChunk<'i> {
    Raw(RawChunk<'i>),
    Transitions(TransitionsChunk<'i>),
}
```

It is the same convention as `AdcStream` (configure with a method, sideband `Open` / `Close`). With `Raw` the link
must carry `rate × width` bits per second; with `Transitions` only the edges, so `StreamCapabilities` gives a
`max_raw_rate` and a `max_rate` (what the sampler can do while the edge rate fits). Every chunk carries its first
sample index, which lines chunks up on the timeline; what happens when the host can't keep up (counting lost
chunks, backfill from device RAM) is `ww-stream-reliability`'s, as for every stream in the family. Until it lands,
a gap in the sample indices is what a host sees.

## Types

```rust
/// A run of raw samples.
pub struct RawChunk<'i> {
    /// Index of the first sample.
    pub first: u32,
    pub samples: u32,
    /// Words as in [RawLayout].
    pub data: MaxLen<RefVec<'i, u8>, MAX_READ_LEN>,
}

/// How raw words are laid out: `width` bits per sample (1, 2, 4, 8, 16, 32).
pub struct RawLayout { pub width: u8 }

/// Edges of one channel in `from..scanned_to`.
pub struct TransitionsChunk<'i> {
    pub channel: u8,
    /// Level at sample `from`.
    pub level: Level,
    pub from: u32,
    /// No more edges before this sample; the next chunk of this channel starts here.
    pub scanned_to: u32,
    /// Samples where the channel toggles, ascending, in `from..scanned_to`.
    pub edges: DeltaOfDelta<'i, u32>,
}

pub enum Encoding { Raw, Transitions }
```

`Level` is `ww_gpio::Level` (1 bit), so the crates share it. `ChannelInfo` (in `LaCapabilities::channels`) has a
`name`, the threshold range if adjustable, and `gpio_pin: Option<u8>`, the index of the same pin in the device's
`ww_gpio::Bank` if it has one.

**Errors**: one plain `LaError` with the family's first four (`Unsupported`, `InUse`, `NoIoLevel`, `Refused`) and
`InvalidArgument`, `WrongMode` (reading a capture that isn't done), `NoSuchChannel`, `Busy` (a stream runs while
a capture is asked for, or the other way round, on devices that share the sampler).

## `PatternGenerator`

```rust
#[ww_trait]
pub trait PatternGenerator {
    fn capabilities() -> PgCapabilities<'i>;

    /// Sets channels, rate, repetition and start condition; returns what was set. Drops a loaded pattern.
    fn configure(config: PgConfig<'i>) -> Result<PgActual, PgError>;

    /// Pattern data, appended in order. `Open` starts a new pattern, `Close` marks it complete.
    #[credit]
    sink!(pattern: PatternChunk<'i>);

    /// Starts output now, or waits for `PgConfig::start`. Playing before the pattern is complete streams it: the
    /// device plays what it has while the sink keeps filling, and stops with `Underrun` if the host falls behind.
    fn start() -> Result<(), PgError>;

    /// Stops output; channels go to their idle levels.
    fn stop();

    fn status() -> PgStatus;
}
```

**Replaying a capture.** `PatternChunk` is `StreamChunk` reused: `Raw(RawChunk)` or `Transitions(TransitionsChunk)`
(each channel's edges, in chunks), the very items `LogicAnalyzer::read_raw` / `read_transitions` and `LogicStream`
return. A host replays a capture by reading it in the encoding the generator accepts (`PgCapabilities::encodings`)
and sending the chunks to the sink, with the capture's `rate` and channel mapping in `PgConfig`. A helper in the
std part converts between the two encodings for devices that take only one.

`Raw` is what a DMA into a GPIO set/reset register plays (STM32 `BSRR`, a PIO `out pins`), so most MCUs take it.
`Transitions` suits devices that time edges with timer compare or a PIO delay loop; for them a sparse pattern at
a high rate fits in little memory.

**Configuration.**

```rust
pub struct PgConfig<'i> {
    /// Output channels ([PgCapabilities::channels]), in the bit order of raw words and as the channel numbers of
    /// transitions.
    pub channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>,
    pub rate: Hertz,
    pub repeat: Repeat,
    /// Levels before the start and after the end, bit `i` for `channels[i]`.
    pub idle: u32,
    pub start: Start,
}

pub enum Repeat { Once, Times(u32), Forever }

pub enum Start {
    /// On `start()`.
    Now,
    /// On the external trigger input.
    External { edge: Edge },
    /// When this device's `LogicAnalyzer` triggers: a stimulus and its response on one time base.
    AnalyzerTrigger,
}
```

`PgStatus`: `Idle`, `Loading { samples }`, `Ready`, `Running { repeat }`, `Done { t_start: Second }`, `Underrun`.
The sink's credit is the device's free pattern memory, so a host can't overrun it; streaming a pattern longer than
the memory works as long as the link keeps up.

**Pins.** While the generator runs, its pins belong to it: `ww_gpio`'s `Pin::set_mode` / `set_output_level` on
them answer `InUse`; reading their level still works. Same rule for analyzer channels that are also outputs
elsewhere: sampling is passive, so a `LogicAnalyzer` may watch pins the device drives (useful to see one's own
output), and `InUse` is only for pins the analyzer would have to reconfigure (a threshold, a pull).

## `WaveformGenerator`

```rust
#[ww_trait]
pub trait WaveformGenerator {
    ww_impl!(channel[]: WaveformChannel);

    fn capabilities() -> WgCapabilities<'i>;

    /// Starts every channel in `channels` on the same sample clock tick, for phase-aligned outputs.
    fn start_together(channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>) -> Result<(), WgError>;
}

#[ww_trait]
pub trait WaveformChannel {
    /// Starts or changes the output, returns what was set (frequency and levels as the DAC can do them).
    fn set(waveform: Waveform) -> Result<WaveformActual, WgError>;

    /// Stops and holds `level`.
    fn stop(level: Volt) -> Result<(), WgError>;

    fn state() -> WaveformState;

    /// Table of an `Arbitrary` waveform, appended in order; `Open` starts a new table, `Close` completes it.
    #[credit]
    sink!(table: MaxLen<RefVec<'i, u8>, MAX_TABLE_CHUNK>);
}

pub struct Waveform {
    pub shape: Shape,
    pub freq: Hertz,
    /// Peak to peak.
    pub amplitude: Volt,
    pub offset: Volt,
    /// 0..=65535 = 0..360°, against the other channels started together.
    pub phase: u16,
}

pub enum Shape {
    Sine,
    Square { duty: u16 },
    Triangle,
    Sawtooth { rising: bool },
    /// `offset` only, `freq` and `amplitude` ignored.
    Dc,
    /// The `table` sink's samples, played once per period (`freq` is the table's repetition rate), or once
    /// and then held when `once` is set.
    Arbitrary { format: SampleFormat, once: bool },
}
```

Amplitudes and offsets are `ww_si::Volt`, frequencies `Hertz` (D2, D3); `WaveformActual` returns what the DAC
actually does (the frequency its sample clock divides to, the clamped levels). A square wave from a timer pin is
`ww_pwm`, not this trait; `WaveformGenerator` is for DACs (an MCU's DAC with a DMA table, a DDS chip, a lab
function generator). Sweeps, bursts, modulation and noise are appendable to `Shape` or as fields with defaults
later.

**Arbitrary tables** are raw DAC codes in a `SampleFormat` like `ww_adc`'s (`U8`, `U16`, `Packed12`, `I16`), with
`WgCapabilities` giving the code-to-volt `Linear` per channel, the same pair `ww_adc` uses the other way round. A
table played with `once` and fed faster than it plays is a streamed DAC output, the reverse of `AdcStream`.

## How it composes with `ww_gpio` and `ww_timebase`

- **`ww_gpio`**: channels of both crates name the same pin's `ww_gpio::Bank` index (`gpio_pin`), so a tool shows
  "LA channel 3 = PA5 = gpio pin 5". The generator takes pins over while running (`InUse` for `ww_gpio`); the
  analyzer only watches. `ww_gpio::Level` is the level type of both.
- **`ww_timebase`**: every timestamp (`CaptureStatus::Done::t_first`, `Triggered::t`, `PgStatus::Done::t_start`)
  is device time from `Clock::now()`'s clock, and the sample clock runs from the same oscillator, so `t_first + k /
  rate` is exact against the rest of the device's data (an I2C transaction from `I2cMonitor`, an ADC capture). Two
  devices synced through `Clock::configure_sync` line their captures up through the `time_map` stream, which is
  enough for "which device saw it first" to a few µs; for sample-exact alignment, wire the trigger out of one to
  the trigger in of the other.
- **`ww_adc`**: a mixed-signal capture is an `AdcCapture` and a `LogicAnalyzer::capture` armed with `External`
  triggers from one source, or `AnalyzerTrigger`-style coupling inside one device later (not in this version).

## Host side (std)

Helpers in each crate's `std` part, not on the wire:

- `Capture`: a whole capture as per-channel edge lists, filled from either encoding, with `level_at(channel,
  sample)`, iteration over edges, conversion back to raw words and to transitions chunks for replay.
- `read_capture(client, config)`: arm, wait, read everything in the best encoding the device lists, as a multi-call
  per round of chunks.
- VCD export (GTKWave, PulseView, sigrok-cli read it): a few dozen lines on top of `Capture`, no dependency.

## Crate and version plan

| Crate | Depends on | Version |
|---|---|---|
| `ww_logic_analyzer` | `wire_weaver`, `ww_si`, `ww_gpio` (`Level`) | 0.1.0, new |
| `ww_signal_generator` | `wire_weaver`, `ww_si`, `ww_gpio`, `ww_logic_analyzer` (chunk types), `ww_adc` (`SampleFormat`, `Linear`) | 0.1.0, new |

Both new, so nothing breaks; skeletons compile `no_std` and `std`, snapshots saved, FEATURES STD-21 / STD-22.

## Open questions

1. **Per-channel transitions, not merged.** The table above says per channel is smaller in every case and
   independent of idle channels; the cost is one call per channel for a full read (one multi-call). Agree?
2. **`DeltaOfDelta<u32>` only, or also `Delta`?** DoD is never much worse than `Delta` here and much better on
   clocked signals. An uneven PWM is the weak case of both (3 % of raw, fine). I'd ship DoD only; a third
   encoding can be appended to `Encoding` if a real capture shows a need.
3. **`ww_adc` dependency for `SampleFormat` / `Linear`.** Reusing them couples `ww_signal_generator` to `ww_adc`;
   the alternative is moving both into `ww_numeric` (STD-23's area). I'd reuse now and move them with STD-23 if it
   touches them anyway.
4. **`rearm()`**: worth a method, or should a host send the configuration again?
5. **Trigger output as a config bool** (and an external trigger input as a `Trigger` variant) is the simplest
   cross-device link. Should trigger in/out instead be a small trait of its own (`ww_timebase`?), shared by
   `AdcCapture`, `LogicAnalyzer` and `PatternGenerator`? It would let one device route triggers between its own
   functions too.

## Not in this version

Protocol, pulse-width and counted triggers; per-channel sample rates (mixed-rate analyzers); on-device protocol
decoding (use the monitor traits); analog channels in a logic analyzer (an `AdcCapture` next to it); sweeps,
bursts and modulation in `WaveformGenerator`; sigrok session (`.sr`) export.
