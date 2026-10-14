# IO trait family 0.2 (design)

!!! warning "Design for review"
    Draft of 14 Oct 2026 (STD-13 io-trait-family), **waiting for Roman's review**. Nothing here is built yet. It
    turns §4 of the rePort API design (`usb_io_fw` `docs/api_design.md`, approved 14 Oct 2026) into exact traits and
    types, settles what §4 left open, and lists where it deviates from §4 ([Decisions](#decisions-for-review)). Once
    approved, each crate is filled in by its own session, and this page becomes the family's overview.

The family: `ww_timebase` (new), `ww_gpio` 0.2, `ww_uart` 0.2, `ww_i2c` 0.2, `ww_spi` 0.2, `ww_adc` (new), `ww_pwm`
(new), `ww_onewire` (new). `ww_files` is not part of this round (WW-5, v1.0).

Goal: one device exposing these traits is driven by generic tools (the `ww` CLI, `ww_*_hl` host crates with
embedded-hal, a GUI) without knowing the device, and every addition the rePort roadmap expects (monitors, PIO
capture, per-byte timestamps, RS-485, slave modes) is wire-compatible later.

## Decisions for review

Recommendations first; each is a question for Roman, the rest of the page follows the recommendation.

| # | Question | Recommendation | Deviates from §4? |
|---|---|---|---|
| D1 | Common errors: one shared error type in a common crate, or the same variants in every crate's own enum? | **Same names and discriminants in every crate's own enum** (`Unsupported`=0, `InUse`=1, `NoIoLevel`=2, `Refused`=3, `InvalidArgument`=4, then the crate's own). No extra crate in the dependency chain; generic tools match by name through introspection. | refines |
| D2 | IO levels: `ww_si::Volt` (gpio, uart today) or millivolts (i2c today)? | **`u16` millivolts everywhere** (`level_mv`), 0 = off. `Volt` is a `NumericValue` + prefix: 3-4 bytes, a float or int a host must interpret, no way to say "off". | yes: `Bank::reference_voltage` becomes `io_level_mv` |
| D3 | UART configuration as properties (`config`, `mode`) or as `configure() -> Actual` like I2C and SPI? | **`configure(UartConfig) -> Result<UartActual, Error>` + `config() -> UartActual`** in all four bus traits. A property can't answer "you asked 115200, you got 115384"; one shape for all buses helps generic tools. | yes: §4.2 had properties |
| D4 | What does `seq` count? | **Units of the stream** (bytes, records, frames, sample sets), wrapping `u32`, lost units included. `next.seq == prev.seq + prev.units + next.lost`, so a gap is exact and visible even if one `lost` is missed. | refines |
| D5 | Lost data: a `Lost` record kind / variant (§4.3, §4.4) or a `lost` field? | **`lost: UNib32` in every stream item**: units dropped right before this item. 4 bits when 0, one rule for every stream, no special records. | yes: drops `RecordKind::Lost`, `FrameFlags::Lost` |
| D6 | Timestamps | **`Ticks(UVlq64)`, absolute, local, in every item** (as §4.5). Delta-coded timestamps inside an item come later as optional appended fields (SW-31). | no |
| D7 | Where the timebase lives on a device without `ww_timebase::Clock` | Items still carry `Ticks`; `capabilities()` of each trait says `timestamps: bool`; without them `t` is 0. A `Clock` anywhere in the tree defines the tick rate (one per device). | refines |
| D8 | Enum growth before SW-16 future-version lands | Every error, mode and kind enum is **`Unsized` with `ww_repr = unib32`** (new variants with data later), and the family ships at 0.2.0 only **after SW-16** (WW-3 ww-evolution), so an old host survives a new error variant. | no (§11.3) |
| D9 | `read_inputs` / `write_outputs` width | `u32` (32 pins per bank), as §4.1. Wider banks are split into banks. | no |
| D10 | Test layout | One test crate pair per trait (`tests/io_gpio` + `tests/io_gpio_api`, ...): parallel fill-ins don't touch each other's files; each mock server is a small loopback simulator that WW-4 (`_hl` crates) reuses. | n/a |

Not decided here, left to the rePort firmware: board-specific rules behind `Refused` and `InUse` (§5), channel lists.

## Conventions for every IO trait

These hold for all crates of the family and for IO traits added later; `docs/api/streams.md` gets the stream part.

**Types.** Every type is `Unsized` unless it is a bit-level leaf (`Level`, `Edge`), `owned(feature = "std")`,
`borrowed` where it has no lifetime, `cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`. Every list,
byte buffer and string is bounded with `MaxLen` (raising a bound later is compatible, lowering is not). Enums that can
grow: `ww_repr = unib32`, not `self_describing`/`sized`. Pins and channels are indices, never MCU pin names.

**Configuration** is one struct argument, `configure(Config) -> Result<Actual, Error>`, returning what was really
set (rounded baud and frequency, the level). `config() -> Actual` reads it back. A config struct gets new fields with
`#[default = ..]` only. `Disabled` is a mode of the config, not a separate method: disabling releases the pins.

**Capabilities** are queried, never assumed: `capabilities() -> XCapabilities<'i>` is resource 0 or close to it in
every trait, and says the ranges, which modes exist, whether timestamps are real, buffer sizes and each stream's
maximum rate alone.

**Errors.** Each crate has one `Error` enum, starting with the common variants at fixed positions:

```rust
#[derive_shrink_wrap(borrowed, owned(feature = "std"), cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
                     derive(Copy, Clone, Debug, PartialEq, Eq), ww_repr = unib32)]
pub enum Error {
    /// Not supported by this device, mode or channel: check `capabilities()`
    Unsupported,       // 0
    /// A pin or peripheral this needs is held by another function (device API says which)
    InUse,             // 1
    /// The translators have no level (IO level off)
    NoIoLevel,         // 2
    /// Board-safety rules don't allow it (device API says why)
    Refused,           // 3
    /// An argument is out of range or inconsistent with the configuration
    InvalidArgument,   // 4
    /// Not configured for this (e.g. a transfer while Disabled, a monitor call in Master mode)
    WrongMode,         // 5
    // crate-specific variants follow (Nack, Timeout, ...), appended later as needed
}
```

Errors stay short and typed (§14 Q14); details come from the device-specific API (`frontend.state()` on rePort).

**Streams carry a head**: every item of a data stream starts with the same three fields, by name, so a generic
recorder handles them all:

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Position of the item's first unit in the stream since it was started, wrapping. Unit per stream: byte (UART), record (I2C monitor), frame (SPI monitor), sample set (ADC), edge (GPIO) |
| `lost` | `UNib32` | Units dropped right before this item (ring full, host too slow). `seq` already counts them |
| `t` | `ww_timebase::Ticks` | Local device time of the item's first event (start bit, start condition, first sample, edge) |

Then the item's own fields, the payload last. Rules (from §10.3): producers never wait; when the ring is full the
newest data is dropped and counted; opening a stream (sideband `Open`) starts nothing, `configure` or a start method
does; closing it stops sending. A host sees a gap as `seq` jumping by `lost`.

**Host → device bulk data** goes through `#[credit] sink!` (API-28); short bus transfers are methods.

**Resource order** is wire-visible forever: the most used resources first (indices 0-7 encode in 4 bits), and
`reserved!` where a trait expects to grow in a group.

## `ww_timebase` (new)

```rust
/// Device time: the one counter that timestamps everything the device captures, at `ClockInfo::tick_hz`
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default),
                     final_structure)]
pub struct Ticks(pub UVlq64); // 5 bytes for the first 9 hours at 1 MHz

#[ww_trait]
pub trait Clock {
    /// 0. Current device time, for host ↔ device correlation (ping-style)
    fn now() -> Ticks;
    /// 1. Tick rate and source
    fn info() -> ClockInfo<'i>;
    /// 2. While synced: (local ticks, shared time) pairs, about once per second
    stream!(time_map: TimeMapPoint);
    /// 3. Sync role and format
    fn configure_sync(config: SyncConfig) -> Result<SyncConfig, Error>;
    fn sync_config() -> SyncConfig;
    fn sync_status() -> SyncStatus;
    /// Time of day a sync master sends (IRIG-B carries it), set from its host
    fn set_time_of_day(t: ww_date_time::DateTime) -> Result<(), Error>;
}

pub struct ClockInfo<'i> {
    pub tick_hz: u64,                    // 1_000_000 on rePort to start
    pub resolution_ticks: u32,           // smallest step actually seen, usually 1
    pub source: MaxLen<&'i str, 32>,     // "rp2350 timer", "dwt", ...
    pub sync_formats: MaxLen<RefVec<'i, SyncFormat>, 8>, // empty: no sync
}
pub enum SyncConfig { Off, Master { format: SyncFormat }, Slave { format: SyncFormat } }
pub enum SyncFormat { IrigB, Pps }       // Q7 of the rePort design decides which rePort does
pub struct SyncStatus { pub state: SyncState, pub offset_ticks: i64, pub last_lock: Option<Ticks> }
pub enum SyncState { Free, Acquiring, Locked, Holdover }
/// `local` ticks correspond to `shared` (master's ticks, or ns of the time of day when known)
pub struct TimeMapPoint { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub shared: u64 }
```

Host helper (in the crate, `std` feature): `TimeMap` that interpolates captured `Ticks` to the shared timeline and
`Ticks::to_duration(tick_hz)`. The device never rewrites timestamps.

## `ww_gpio` 0.2

Kept from 0.1: `Bank` with `pin[]`, `Pin` with its first four methods at indices 0-3 (`set_output_level`, `toggle`,
`output_level`, `input_level`) and `event` at 7, `Level`, `Mode`, `Pull`, `Speed`. Changed and added:

```rust
#[ww_trait]
pub trait Bank {
    ww_impl!(pin[]: Pin);                    // 0
    /// 1. Input levels of all pins at once, bit n = pin n
    fn read_inputs() -> u32;
    /// 2. Output levels of the pins in `mask`, set at the same instant where the hardware can
    fn write_outputs(mask: u32, levels: u32) -> Result<(), Error>;
    reserved!(3..=7);
    fn capabilities() -> BankCapabilities<'i>;
    /// IO level of the bank in mV (0 = off); writes go through the device's level checks (D2)
    property!(rw io_level_mv: u16, Error);
    fn name() -> MaxLen<&'i str, 16>;
}

#[ww_trait]
pub trait Pin {
    fn set_output_level(level: Level);       // 0  (unchanged; errors as before are silent)
    fn toggle();                             // 1
    fn output_level() -> Level;              // 2
    fn input_level() -> Level;               // 3
    reserved!(4..=6);
    stream!(event: TimedPinEvent);           // 7  now timestamped (was IoPinEvent)
    fn set_mode(mode: Mode, initial: Option<Level>) -> Result<(), Error>;
    fn mode() -> Mode;
    property!(rw pull: Pull, Error);
    property!(rw speed: Speed, Error);
    fn configure_events(events: PinEvents) -> Result<(), Error>;
    /// What this pin can do
    fn capabilities() -> PinCapabilities<'i>;
}

pub struct TimedPinEvent { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub edge: Edge, pub level_after: Level }
pub enum Edge { Rising, Falling }            // sized, u1
pub struct PinEvents { pub rising: bool, pub falling: bool, pub debounce_us: u32 }
pub struct PinCapabilities<'i> {
    pub modes: MaxLen<RefVec<'i, Mode>, 8>, pub pulls: MaxLen<RefVec<'i, Pull>, 4>,
    pub speeds: MaxLen<RefVec<'i, Speed>, 4>,
    pub dir_switch_ns: u32,                  // how long a direction change takes (open drain emulation)
    pub events: bool, pub event_timestamps: bool, pub max_event_rate_hz: u32,
    pub name: MaxLen<&'i str, 16>,           // "IO4"
}
pub struct BankCapabilities<'i> {
    pub io_levels_mv: MaxLen<RefVec<'i, u16>, 8>, pub io_level_range_mv: Option<(u16, u16)>,
    pub pin_count: u8, pub atomic_write_outputs: bool,
}
```

`event` changes type in place (0.2 breaks the wire anyway, D2), which keeps it at index 7. `IoPinEnabledEvents` with
its unbounded `custom` list is gone (custom events: a later appended field). `ww_gpio_hl` follows (same session).

## `ww_uart` 0.2

```rust
#[ww_trait]
pub trait Uart {
    stream!(rx: RxChunk<'i>);                                     // 0
    #[credit] sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);    // 1  UART_TX_MAX = 256
    stream!(monitor: MonitorChunk<'i>);                           // 2  Mode::Monitor2
    fn capabilities() -> Capabilities<'i>;                        // 3
    fn configure(config: UartConfig) -> Result<UartActual, Error>;
    fn config() -> UartActual;
    fn send_break(duration_us: u32) -> Result<(), Error>;
    /// Mode::Test only, outputs only
    fn set_pin_level(pin: Pin, high: bool) -> Result<(), Error>;
    /// Bytes waiting in the TX buffer (for a host that wants to know when TX drained)
    fn tx_pending() -> u32;
}

pub struct UartConfig {
    pub mode: Mode,                 // Disabled, Normal, Monitor2, Test
    pub baud: u32,
    pub data_bits: DataBits,        // Five..Nine
    pub parity: Parity,             // None, Odd, Even, Mark, Space
    pub stop_bits: StopBits,        // One, OneAndHalf, Two
    pub flow: FlowControl,          // None, RtsCts, XonXoff
    pub idle_bits: u16,             // chunk ends after this much silence; 0 = default (1.5 characters)
    pub max_chunk: u16,             // 0 = device default
    pub io_level_mv: Option<u16>,   // None = leave the level as it is (D2)
    pub prevent_back_feed: bool,    // MON-4, if capabilities say so
}
pub struct UartActual { pub config: UartConfig, pub baud_actual: u32 }
pub struct RxChunk<'i> {
    pub seq: u32, pub lost: UNib32, pub t: Ticks,
    pub framing: bool, pub parity: bool, pub overrun: bool, pub break_: bool,
    pub bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX>,         // 512
}
pub struct MonitorChunk<'i> { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub line: Line,
                              pub framing: bool, pub parity: bool, pub overrun: bool, pub break_: bool,
                              pub bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX> }
pub enum Line { A, B }              // rePort: A = RX, B = CTS / RX2
pub enum Pin { Tx, Rx, Rts, Cts }
pub struct Capabilities<'i> {
    pub baud_min: u32, pub baud_max: u32, pub modes: MaxLen<RefVec<'i, Mode>, 8>,
    pub data_bits: MaxLen<RefVec<'i, DataBits>, 5>, pub flow: MaxLen<RefVec<'i, FlowControl>, 3>,
    pub io_levels_mv: MaxLen<RefVec<'i, u16>, 8>, pub timestamps: bool, pub back_feed_detector: bool,
    pub rx_buffer: u32, pub tx_buffer: u32,
}
```

`Monitor4` and `HighZ` are gone (`Disabled` = high-Z); `tx_mon` is gone (the monitor stream covers it). The monitor
`seq` counts bytes of both lines together. Error-specific variants: `BufferFull` (only for writes outside the credit
sink), `Timeout`.

## `ww_i2c` 0.2

As §4.3, with D1, D3, D5 applied:

```rust
#[ww_trait]
pub trait I2c {
    fn write_read(addr: Address, write: MaxLen<RefVec<'i, u8>, 64>, read_len: u16) -> Result<ReadResult<'i>, Error>; // 0
    fn write(addr: Address, data: MaxLen<RefVec<'i, u8>, I2C_MAX>) -> Result<Timing, Error>;                       // 1
    fn read(addr: Address, len: u16) -> Result<ReadResult<'i>, Error>;                                               // 2
    fn transaction(addr: Address, ops: MaxLen<RefVec<'i, Op<'i>>, 16>) -> Result<TransactionResult<'i>, Error>;     // 3
    stream!(monitor: MonitorRecord<'i>);                                                                             // 4
    reserved!(5..=7);
    fn capabilities() -> Capabilities<'i>;
    fn configure(config: I2cConfig) -> Result<I2cActual, Error>;
    fn config() -> I2cActual;
    fn scan(range: AddrRange) -> Result<MaxLen<RefVec<'i, u8>, 128>, Error>;
    fn lines() -> LinesState;
    fn recover() -> Result<LinesState, Error>;
}
pub enum Address { Seven(u7), Ten(u10) }
pub struct I2cConfig { pub mode: I2cMode, pub speed_hz: u32, pub io_level_mv: Option<u16>,
                       pub pullups: Pullups, pub stretch_timeout_us: u32 }
pub enum I2cMode { Disabled, Master { monitor: bool }, Slave { addr: Address, monitor: bool }, MonitorOnly }
pub enum Pullups { Off, On, Keep }
pub enum Op<'i> { Write(MaxLen<RefVec<'i, u8>, I2C_MAX>), Read(u16) }
pub struct Timing { pub t_start: Ticks, pub t_end: Ticks }
pub struct ReadResult<'i> { pub timing: Timing, pub data: MaxLen<RefVec<'i, u8>, I2C_MAX> }
pub struct TransactionResult<'i> { pub timing: Timing, pub reads: MaxLen<RefVec<'i, MaxLen<RefVec<'i, u8>, I2C_MAX>>, 16> }
pub struct AddrRange { pub first: u7, pub last: u7, pub include_reserved: bool }
pub struct LinesState { pub scl_high: bool, pub sda_high: bool, pub scl_mv: Option<u16>, pub sda_mv: Option<u16> }
pub struct MonitorRecord<'i> { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub t_end: Ticks, pub addr: Address,
                               pub read: bool, pub kind: RecordKind, pub nack_at: Option<u16>,
                               pub bytes: MaxLen<RefVec<'i, u8>, I2C_MAX> }
pub enum RecordKind { Complete, RepeatedStart, BusError }
// I2C_MAX = 512; crate errors: Nack { at: u16 }, AddrNack, Arbitration, Timeout, BusStuck
```

`write_read` first: the most common call of a register-based sensor. Cycle reads and delays of the 0.1 prototype
become IF-6 i2c-polling later.

## `ww_spi` 0.2

```rust
#[ww_trait]
pub trait Spi {
    fn transfer(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, Error>; // 0
    fn transaction(ops: MaxLen<RefVec<'i, Op<'i>>, 16>) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, Error>; // 1
    fn write(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<(), Error>;                                   // 2
    stream!(monitor: Frame<'i>);                                                                            // 3
    reserved!(4..=7);
    fn capabilities() -> Capabilities<'i>;
    fn configure(config: SpiConfig) -> Result<SpiActual, Error>;
    fn config() -> SpiActual;
    fn set_cs(active: bool) -> Result<(), Error>;          // CsMode::Manual
}
pub struct SpiConfig { pub role: Role, pub freq_hz: u32, pub mode: SpiMode, pub bit_order: BitOrder,
                       pub word_bits: u8, pub cs: CsMode, pub io_level_mv: Option<u16> }
pub enum Role { Disabled, Master, Slave, Monitor }
pub enum SpiMode { Mode0, Mode1, Mode2, Mode3 }
pub enum BitOrder { MsbFirst, LsbFirst }
pub enum CsMode { Auto, Manual, None }
pub enum Op<'i> { Write(MaxLen<RefVec<'i, u8>, SPI_MAX>), Read(u16), Transfer(MaxLen<RefVec<'i, u8>, SPI_MAX>), DelayUs(u16) }
pub struct Frame<'i> { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub t_end: Ticks, pub truncated: bool,
                       pub last_bits: u8, pub mosi: MaxLen<RefVec<'i, u8>, SPI_MAX>, pub miso: MaxLen<RefVec<'i, u8>, SPI_MAX> }
// SPI_MAX = 512; transaction returns the read bytes of Read and Transfer ops, concatenated
```

`word_bits` > 8: words little-endian in 2 bytes. Slave mode data paths come with IF-5 (additive: methods appended).

## `ww_adc` (new)

As §4.6, with the conventions applied:

```rust
#[ww_trait]
pub trait Adc {
    fn read(channel: u8) -> Result<Reading, Error>;                                  // 0
    fn read_many(mask: u32) -> Result<MaxLen<RefVec<'i, Reading>, 32>, Error>;       // 1
    stream!(samples: SampleBlock<'i>);                                              // 2
    reserved!(3..=7);
    fn capabilities() -> Capabilities<'i>;
    fn start_stream(config: StreamConfig) -> Result<StreamActual, Error>;
    fn stop_stream();
    fn capture(config: CaptureConfig) -> Result<CaptureActual, Error>;
    fn capture_status() -> CaptureStatus;
    fn capture_read(offset: u32, len: u16) -> Result<MaxLen<RefVec<'i, u8>, ADC_READ_MAX>, Error>; // 768
    fn capture_abort();
}
pub struct Reading { pub channel: u8, pub raw: u16, pub value: f32, pub t: Ticks }
pub struct ChannelInfo<'i> { pub name: MaxLen<&'i str, 16>, pub unit: Unit, pub min: f32, pub max: f32,
                             pub bits: u8, pub calibrated: bool }
pub enum Unit { Volt, Ampere, Celsius, Raw }   // grows
pub struct Capabilities<'i> { pub channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, 32>, pub max_rate_hz: u32,
                              pub capture_max_samples: u32, pub formats: MaxLen<RefVec<'i, SampleFormat>, 4>,
                              pub simultaneous: bool }
pub struct StreamConfig { pub mask: u32, pub rate_hz: u32, pub format: SampleFormat, pub flush_ms: u16 }
pub struct StreamActual { pub config: StreamConfig, pub rate_hz_actual: u32 }
pub enum SampleFormat { U16, Packed12 }
/// `seq` counts sample sets (one sample of every channel in `mask`); `data` interleaved in mask order
pub struct SampleBlock<'i> { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub mask: u32, pub format: SampleFormat,
                             pub data: MaxLen<RefVec<'i, u8>, ADC_BLOCK_MAX> }  // 512
pub struct CaptureConfig { pub mask: u32, pub rate_hz: u32, pub samples: u32, pub format: SampleFormat,
                           pub trigger: Trigger, pub pre_trigger: u32 }
pub enum Trigger { Now, Rising { channel: u8, level: f32 }, Falling { channel: u8, level: f32 }, Sync }
pub struct CaptureActual { pub config: CaptureConfig, pub rate_hz_actual: u32, pub bytes: u32 }
pub enum CaptureStatus { Idle, Armed, Triggered { t: Ticks }, Done { t_trigger: Ticks, t_first: Ticks, samples: u32 } }
```

Calibration converts `raw` to `value` on the device; samples in streams and captures stay raw (host converts with
per-channel `scale` / `offset`, a later appended `ChannelInfo` field: a design gap to close in the fill-in).

## `ww_pwm` (new)

```rust
#[ww_trait]
pub trait Pwm {
    ww_impl!(channel[]: Channel);                                // 0
    fn capabilities() -> Capabilities<'i>;
}
#[ww_trait]
pub trait Channel {
    fn set_output(output: PwmOutput) -> Result<PwmActual, Error>;  // 0
    fn set_duty(duty: u16) -> Result<u16, Error>;                  // 1 duty only, frequency kept (fast path)
    fn stop(idle: ww_gpio::Level) -> Result<(), Error>;            // 2
    stream!(capture: CaptureSample);                               // 3
    reserved!(4..=7);
    fn set_capture(config: CaptureConfig) -> Result<CaptureConfig, Error>;
    fn read_capture() -> Result<CaptureSample, Error>;
    fn state() -> ChannelState;
}
pub struct PwmOutput { pub freq_hz: u32, pub duty: u16 /* 0..=65535 */, pub inverted: bool, pub phase_correct: bool }
pub struct PwmActual { pub freq_hz: f32, pub duty: u16 }
pub struct CaptureConfig { pub mode: CaptureMode, pub gate_us: u32, pub stream: bool }
pub enum CaptureMode { Off, Frequency, DutyCycle, PulseWidth, EdgeCount }
pub struct CaptureSample { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub value: f64 /* Hz, 0..1, s, count */ }
pub enum ChannelState { Idle, Output(PwmActual), Capture(CaptureConfig) }
pub struct ChannelInfo<'i> { pub name: MaxLen<&'i str, 16>, pub group: u8, pub output: bool, pub capture: bool }
pub struct Capabilities<'i> { pub channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, 32>, pub freq_min_hz: u32,
                              pub freq_max_hz: u32, pub duty_bits: u8 }
// crate error: SharedTimer (the group's other channel runs at another frequency)
```

## `ww_onewire` (new)

```rust
#[ww_trait]
pub trait OneWire {
    fn transaction(t: Transaction<'i>) -> Result<MaxLen<RefVec<'i, u8>, OW_MAX>, Error>;   // 0
    fn reset() -> Result<bool, Error>;                                                     // 1 presence
    fn search(start: SearchState, family: Option<u8>) -> Result<SearchPage<'i>, Error>;   // 2 paged
    reserved!(3..=7);
    fn capabilities() -> Capabilities<'i>;
    fn configure(config: OneWireConfig) -> Result<OneWireConfig, Error>;
    fn config() -> OneWireConfig;
    fn triplet(direction: bool) -> Result<Triplet, Error>;   // bit-level search on the host
}
pub struct RomId(pub [u8; 8]);
pub struct OneWireConfig { pub pin: u8 /* index into capabilities().pins */, pub speed: Speed, pub enabled: bool }
pub enum Speed { Standard, Overdrive }
pub struct Transaction<'i> { pub rom: Option<RomId> /* None = skip ROM */, pub write: MaxLen<RefVec<'i, u8>, OW_MAX>,
                             pub read_len: u16, pub strong_pullup_ms: u16 }
pub struct SearchState { pub last_rom: Option<RomId>, pub last_discrepancy: u8 }
pub struct SearchPage<'i> { pub roms: MaxLen<RefVec<'i, RomId>, 16>, pub next: Option<SearchState> }
pub struct Triplet { pub id_bit: bool, pub cmp_bit: bool, pub direction: bool }
pub struct Capabilities<'i> { pub pins: MaxLen<RefVec<'i, MaxLen<&'i str, 16>>, 8>, pub overdrive: bool,
                              pub strong_pullup: bool }
// OW_MAX = 256; crate errors: NoPresence, CrcMismatch, ShortCircuit
```

## Versions, compatibility, what breaks

- `ww_gpio`, `ww_uart`, `ww_i2c`, `ww_spi` 0.1 → 0.2: **wire and Rust API breaks**. Affected in this repo:
  `ww_gpio_hl`, `examples/all_gpio*`, `examples/uart*`, `examples_mcu/{mcu_qemu, nucleo_h743zi2,
  usb_stm32h725ig}`; outside: `usb_io_fw` (moves to the family in its 0.4.0, FW-1). No deployed device besides rePort
  rev A boards in-house uses them.
- New crates start at 0.1.0. All of them (and `ww_timebase` as everyone's dependency) are snapshotted
  (`just save-snapshots`); `ww_timebase` is a dependency of every other crate, so a break there cascades: it is
  kept tiny.
- Release order: SW-16 future-version (WW-3) first, then the family at once, so no crate ships an enum an old reader
  chokes on.
- Canonical home is `vhrdtech/ww_stdlib`: the new crates get copied there at release, as the existing ones.

## Implementation plan

1. **Skeleton (this session, after review):** all eight crates with the traits and types above, compiling `no_std`
   and `std`, workspace wiring, `ww_timebase` complete (it is small and everyone needs `Ticks`), stream conventions in
   `docs/api/streams.md`, FEATURES items. One commit, so the fill-ins branch from it and never touch each other's
   files.
2. **Fill-ins on sonnet, in parallel** (`--from-here`), one per crate: rustdoc on every item, host helpers (`std`),
   the test pair `tests/io_<x>` with a loopback mock server driven through `tests_common::start_device`, a docs page
   `docs/std_library/ww_<x>.md` + nav, snapshot, CHANGELOG, FEATURES. `ww_gpio` also moves `ww_gpio_hl` and the
   examples; `ww_uart` the uart example. Done when `just check`, `just test`, clippy and `just build-docs` pass.
3. **Review** of the whole family by Roman, then `tpm land` in one go.

Risks the skeleton checks first: `u7` / `u10` inside enum variants and `RefVec` of `MaxLen<RefVec<..>>` in method
returns through the codegen (no test uses them yet); `#[credit] sink!` of a stdlib trait used by `ww_impl!`; tuple
`Option<(u16, u16)>` in a struct (the gpio 0.1 code has a TODO saying tuples were not supported: fall back to two
fields).

## Ideas not asked for

- **Loopback mocks as a reusable simulator** (`ww_io_sim`, std): the test servers of step 2, wired TX↔RX,
  IO0↔IO4, an I2C EEPROM at 0x50, a 1-wire DS18B20; WW-4 runs crates.io embedded-hal drivers against it and the
  rePort sim (HOST-2) reuses it.
- **`ww io` CLI subcommands** (TOOL-8 and friends): `ww gpio`, `ww uart term`, `ww i2c scan`, `ww adc read` on any
  device that has the trait, found by global trait path (needs API-20, WW-2).
- **sigrok export** of monitor streams on the host (`seq` / `t` / `lost` map to it directly).
