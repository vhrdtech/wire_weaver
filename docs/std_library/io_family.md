# IO trait family (design)

!!! note "Approved, being built"
    STD-13 io-trait-family, approved by Roman on 14 Oct 2026 after three review rounds (answers in
    [Review answers](#review-answers)). Every crate exists as a skeleton on the branch (the sources and their doc
    comments are the API description); the per-crate fill-ins add docs pages, tests and host helpers.

## Scope and principles

The family is for **every device** that exposes IO over WireWeaver: a Nucleo board bridging its pins to a PC, a USB
adapter like an FTDI or Bus Pirate, a motor controller, an ESP32 sensor node, a lab instrument, rePort. rePort is the
first user and the source of the requirements, not the shape of the traits.

1. **What deserves a trait is a trait.** A separate function a device may or may not have (a bus monitor, ADC
   streaming, ADC capture, PWM capture) is its own trait; a few methods some devices can't do (I2C bus recovery,
   switchable pull-ups) stay in the core trait and answer `Unsupported`. That such methods still cost a dispatch
   entry and a stub is solved in the framework, with cfg-style features on trait methods (`ww-errors-plan`), not by
   splitting traits. Bigger subjects get their own crates: `ww_logic_analyzer`, `ww_signal_generator`.
2. **Evolve what works.** `ww_gpio` (usable, used), `ww_uart` and `ww_i2c` (prototypes) keep every resource, index,
   name and type that works; each section lists what is kept, changed (and why) and added.
3. **Typed values through `ww_si`.** Voltages, frequencies and timestamps are `ww_si` quantities, not `u16` millivolts
   or a new tick type. The device reports in whatever storage is convenient for it (a `u32` of µs, an `f32` of V);
   the host reads them all. `ww_si` and `ww_numeric` get extended for it, every part behind a cargo feature
   ([below](#ww_si-and-ww_numeric-improvements)).
4. **Ergonomic first, fast paths separate.** `ww_gpio` stays low-speed, logical-pin GPIO. Sampling many pins fast
   is a logic analyzer, driving many pins fast is a signal generator. Many small calls in one round trip are
   multi-call (CLT-15), a framework feature, not a method per trait.
5. **No bodges from the side.** What is a framework concern (reliable streams, error evolution, rustdoc) is solved in
   the framework by its own session; the family leaves a pointer and stays plain until it lands.

## Moved to other work

| Subject | Where | What the IO family does meanwhile |
|---|---|---|
| Stream reliability: sequence numbers, lost counts, backfill (e.g. from an SD card), stream control through sideband commands, per-chunk timestamps | `ww-stream-reliability` | No `seq` / `lost` in stream items; streams start and stop with sideband `Open` / `Close`; items carry only the timestamps that belong to the data (an I2C transaction's start and end) |
| `#[future_version]` by default (SW-47), per-method errors, common + crate-specific variants, errors carrying strings or bytes | `ww-errors-plan` | Plain error enums: 0.1 variants kept, `InUse`, `NoIoLevel`, `Refused`, `Unsupported` appended as unit variants; no hand-typed `#[future_version]` |
| Methods a device never supports, compiled out (cfg-style features on trait methods) | `ww-errors-plan` | Such methods answer `Unsupported`; capabilities say what works |
| `#[ww_trait]` in rustdoc | `ww-rustdoc-traits` (DOC-6) | Crate docs name traits and methods in backticks, not as links |

## Crates and traits

| Crate | Traits | Status |
|---|---|---|
| `ww_gpio` | `Bank`, `Pin` | evolves 0.1, additive except errors |
| `ww_uart` | `Uart`, `UartMonitor` | evolves 0.1 |
| `ww_i2c` | `I2c`, `I2cWatch`, `I2cMonitor` | evolves 0.1, **written** |
| `ww_spi` | `Spi`, `SpiMonitor` | new content (0.1 is empty) |
| `ww_adc` | `Adc`, `AdcStream`, `AdcCapture` | new, **written** |
| `ww_pwm` | `Pwm` with `channel[]: PwmChannel`, `PwmCapture` | new |
| `ww_onewire` | `OneWire` | new |
| `ww_timebase` | `Clock` | new: device time and sync, no new time type |
| `ww_logic_analyzer` | `LogicAnalyzer`, `LogicStream` | new |
| `ww_signal_generator` | `PatternGenerator`, `WaveformGenerator` | new |
| `ww_si` / `ww_numeric` | — | extended for the above |

A device picks traits, for example a Nucleo bridge: `gpio: ww_gpio::Bank`, `uart: ww_uart::Uart`,
`i2c: ww_i2c::I2c`; rePort adds `uart_monitor: ww_uart::UartMonitor`, `i2c_monitor: ww_i2c::I2cMonitor`,
`la: ww_logic_analyzer::LogicAnalyzer`, and so on. Each trait sits at most once in a device's tree and outside trait
arrays, so generic tools find it by global trait path (API-20, CLT-10, landed).

## Review answers

### Round 3

**10-bit addresses without the extra byte.** Done: SW-48 (bit-width fields in the derive, child session
`ww-subbyte-fields`) packs `I2cAddress { Seven(U7), Ten(U10) }` into 8 bits for a 7-bit address and 11 for a
10-bit one, so `ww_i2c` supports both at no cost.

**A third `ww_i2c` trait for autonomous reads.** `I2cWatch` (usb_io_fw IF-6 i2c-polling): the device repeats
configured reads (`I2cWatchConfig`: address, plain or register read, length, period) and reports on the `changes`
stream only what `I2cWatchReport` asks for: every read, a change under a mask, or a mask / value match starting or
ending. Host transactions have priority and run between watch reads; errors are reported once. Slots reuse the 0.1
`OutOfCycleSlots` / `InvalidCycleSlot` errors meant for cycle reads.

**Approved otherwise.** Errors follow `ww-errors-plan` (approved there: `Common(CommonError)` first, `#[errors(..)]`
per method, SW-47 automatic `Unknown`) through its stdlib migration, which covers every crate here; `seq` / `lost` /
backfill wait for `ww-stream-reliability`.

### Round 2

**D4 `seq` / D5 `lost`: no bodging reliable transfer into each trait.** Agreed. Both fields are out of every item
type; `I2cRecord`, `SampleBlock` and the UART chunks wait for `ww-stream-reliability`. On USB nothing should be
lost; what remains are slow hosts on fast monitors and lossy links (UDP, LoRa), which that work covers.

**D7: every device has a timer; no `Option` where it isn't needed.** Agreed. Timestamps are plain `Second`, never
`Option`, and the `timestamps: bool` capability flags are gone. The `Option`s left are real choices that cost one
bit: `delay_after` (no delay), `io_level` (leave it as it is), `flush_interval` (device default), `t_trigger` (no
trigger), measured line voltages (most adapters can't measure).

**D8: `#[future_version]` by hand everywhere.** Removed from both crates; making it automatic (SW-47) is
`ww-errors-plan`'s.

**Errors: per-method, common + own, carrying strings.** Moved to `ww-errors-plan`. The family's enums are plain:
`I2cError` is the 0.1 enum plus `InUse`, `NoIoLevel`, `Refused`, `Unsupported`; `AdcError` is new with the same
four first. Whatever `ww-errors-plan` decides is applied once, to all crates.

**Methods a device never supports cost a stub; don't split traits just for that.** Agreed, and corrected: `I2cBus`
(lines, recovery, pull-ups) is merged back into `I2c` as `i2c_lines_state`, `i2c_wiggle_clock`, `i2c_set_pullups`,
`i2c_pullups`, the names the 0.1 `TODO`s had. Monitors, ADC streaming, ADC capture and PWM capture stay traits:
each is a function of its own that many devices lack entirely, with its own streams and state, and a sniffer can
implement `I2cMonitor` without having an I2C master.

**`ww_numeric` / `ww_si`: extending them is welcome, everything a cargo feature.** Yes: number types already are
features; the plan extends that to quantity groups and the host helpers
([below](#ww_si-and-ww_numeric-improvements)).

**`ww_i2c`: 10-bit addresses?** **Support them**, as `I2cAddress { Seven(u8), Ten(u16) }`. Why: they are in the
specification, embedded-hal's `I2c` trait is generic over `SevenBitAddress` / `TenBitAddress` (so the host crate
`ww_i2c_hl` maps cleanly onto both), and adding them later would break every method's signature. The cost, measured
with a scratch serializer on this branch: a write-read request's address part is 2 bytes instead of 1 (`[00, 50]`
for `Seven(0x50)`, `[80, 50, 03]` for `Ten(0x350)`), because the variant bit is padded to a byte unless another
bit-sized argument shares it, and the derive macro can't pack a `u7` field yet. With bit-width fields in the
derive (`Seven(u7)`), a 7-bit address would be one byte again with no API change on the Rust side; that is a
`shrink_wrap` item worth filing. An adapter without 10-bit support answers `Unsupported` and says so in
`I2cCapabilities::ten_bit_addresses`. `i2c_scan` still returns the 7-bit addresses (a 10-bit scan is rarely
useful and would be a 1024-address probe).

**`ww_adc::stop_stream`: why not a sideband command?** Done: `AdcStream::configure_stream` sets the configuration
(and answers with the actual rate), sideband `Open` starts sampling, `Close` stops it; `start_stream` and
`stop_stream` are gone. One gap is noted for `ww-stream-reliability`: `Open` can't carry the configuration, so a
host configures first, then opens.

**rustdoc for `#[ww_trait]`.** `ww-rustdoc-traits` (DOC-6); `ww_i2c` and `ww_adc` are good test cases for it.

### Round 1, what still stands

- **For all devices, not only rePort; own crates for logic analysis and signal generation**: as above.
- **`cnt` from the start**: a task for `examples_mcu/*` and the test firmwares (counters in the MCU examples,
  `runner = "cnt run --chip ..."` in each board's `.cargo/config.toml`, as `usb_io_fw` does).
- **D2 levels as `ww_si::Volt`**, never `u16` millivolts: `Volt` lets the device pick the storage (`U16` of mV,
  `F32` of V); its prefix and type tag cost one byte, irrelevant for configuration calls.
- **D3 methods returning the actual value set** for settings the hardware rounds; plain choices stay properties.
- **D6 timestamps as `ww_si::Second`**: device time since its clock started, in the prefix and storage the device
  picks (`U32` of µs on most MCUs: 5 bytes with prefix and tag; `U64` where there is a 64-bit counter). A fixed-width
  value wraps (a `U32` of µs every 71.6 minutes) and the host unwraps it; `Clock::now()` gives the full value. One
  clock per device, so its data lines up across traits. No `Ticks` type, no `UVlq64` (its size grows with uptime).
- **D9 no `u32` pin masks** in `ww_gpio`: pins by logical index only; bulk is multi-call, precise multi-pin timing
  is a logic analyzer or signal generator.
- **D10 one test crate pair per trait** (`tests/io_<trait>` + `tests/io_<trait>_api`), each mock server doubling as
  a simulator for host crates (WW-4).
- **`TimedPinEvent` removed** from `ww_gpio`; its 0.1 `event` stream stays for low-speed edges.
- **`read_many` removed** from `ww_adc`; simultaneous sampling is a one-set capture in `AdcCapture`.
- **Mocks, `ww` subcommands, sigrok export, rerun**: yes, in [ideas](#ideas-not-asked-for).

## Conventions for every IO trait

**Types.** New types are `Unsized` unless they are bit-level leaves, `owned(feature = "std")`,
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`. Every list, byte buffer and string is bounded with
`MaxLen` (raising a bound later is compatible, lowering is not). Physical values are `ww_si` quantities. No `Option`
unless "none" is a real value.

**Configuration.** Plain choices are properties. Settings the hardware rounds are methods returning what was set
(`i2c_configure` returns the real frequency, `AdcStream::configure_stream` the real rate). A trait with many coupled
settings takes one config struct (new fields with `#[default = ..]` only); `Disabled` is a mode, disabling releases
the pins.

**Errors.** One plain enum per crate; `InUse` (pins or peripheral serve another function), `NoIoLevel` (level
translators unpowered), `Refused` (a safety rule of the device), `Unsupported` (this device doesn't have it) by these
names in each. Each method's doc lists the variants it returns in an `# Errors` section. Evolution of errors:
`ww-errors-plan`.

**Streams.** Opening a stream starts nothing by itself unless its trait says so; streams with a configuration are
configured with a method and started and stopped with sideband `Open` / `Close`. Host → device bulk data goes
through `#[credit] sink!` (API-28). Loss reporting and per-chunk timestamps: `ww-stream-reliability`.

**Capabilities** are queried, never assumed: every core trait has `capabilities()`.

**Resource order** is wire-visible: the most used resources first (indices 0-7 encode in 4 bits).

## `ww_gpio`

0.1 is usable and used (`ww_gpio_hl`, `examples/all_gpio`, three `examples_mcu` boards). The new version is 0.1 plus
a per-pin `capabilities()` and the common errors; everything 0.1 sends keeps its bytes.

| | Item | Why |
|---|---|---|
| kept | `Bank`: `pin[]`, `reserved!(1..=7)`, `capabilities()`, `reference_voltage: Volt`, `name()` | works |
| kept | `Pin`: `set_output_level` 0, `toggle` 1, `output_level` 2, `input_level` 3, `event` 7, `set_mode`, `mode`, `pull`, `speed`, `configure_events` | works, indices are wire |
| kept | `Level`, `Mode`, `Pull`, `Speed`, `IoPinEvent`, `IoPinEnabledEvents`, `BankCapabilities`, all `Error` variants | works |
| added | `Pin::capabilities() -> PinCapabilities` (appended): modes, pulls, speeds, `name`, whether events work | which pin can do what differs per pin on most MCUs and adapters |
| added | `Error`: `InUse`, `NoIoLevel`, `Refused` (appended) | common errors |

## `ww_uart`

0.1 is a prototype used by `examples/uart`. The new version keeps the trait shape and its properties, changes the
TX sink, and moves monitoring into its own trait.

| | Item | Why |
|---|---|---|
| kept | `rx` stream at 0, `capabilities()`, properties `baud_rate`, `mode`, `stop_bits`, `parity`, `prevent_back_feed`, `reference_voltage`, `set_pin_level` | work |
| kept | `BaudRate`, `Mode`, `Pin`, `StopBits`, `Parity`, `Capabilities` fields, `Error` variants | work |
| kept | `RxChunk { flags, timestamp, bytes }` for now, `bytes` bounded | its time and loss fields are `ww-stream-reliability`'s |
| changed | `sink!(tx: [u8])` → `#[credit] sink!(tx: MaxLen<RefVec<'i, u8>, 256>)` | a plain sink overruns the device's TX buffer on bulk writes (a file sent to the UART); credit sinks (API-28) exist for this |
| moved | `tx_mon` → `UartMonitor` trait: `line_a` / `line_b` streams of `RxChunk` | a monitor is its own function; most UARTs can't do it |
| added | `data_bits` property, `set_baud(BaudRate) -> Result<u32, Error>` returning the real baud, `send_break`, `tx_pending` | missing settings; D3 |
| added | `RxFlags`: `framing_error`, `overrun`, `break_detected` (appended with defaults) | error flags |
| added | `Error`: `InUse`, `NoIoLevel`, `Refused`, `Timeout` | |

## `ww_i2c`

**Written** (`ww_stdlib/ww_i2c/src/lib.rs`). Against 0.1:

| | Item | Why |
|---|---|---|
| kept | `I2c`: `i2c_capabilities`, `i2c_configure`, `i2c_write`, `i2c_read` with `I2cReadKind`, `i2c_scan`, `delay_after`, names and order; `I2cMode`; `I2cError` variants | work |
| changed | `addr: u8` → `I2cAddress { Seven(U7), Ten(U10) }` | 10-bit support at no cost, see round 3 |
| changed | `io_level_mv: u16` → `io_level: Option<Volt>` | D2; `None` leaves the level |
| changed | `Vec<u8>` → `MaxLen<RefVec<'i, u8>, N>` | a `no_alloc` device server can't take a `Vec`; bounded sizes |
| changed | `RawTimestamp { ticks: u32 }` → `ww_si::Second` | D6 |
| changed | `delay_after: u32` (no unit) → `Option<Second>` | typed; 1 bit when there is no delay |
| changed | plain structs get `#[derive_shrink_wrap]` | they cross the wire |
| added | `i2c_transaction` | repeated starts, embedded-hal `transaction` |
| added | `i2c_lines_state`, `i2c_wiggle_clock`, `i2c_set_pullups`, `i2c_pullups` | the 0.1 `TODO`s: see a stuck bus, free it |
| added | `I2cWatch` trait: autonomous reads reported by change or match | the host doesn't poll (IF-6) |
| added | `I2cMonitor` trait: one `I2cRecord` per transaction (start, end, address, direction, bytes, NACK position) | sniffers |
| added | `I2cError`: `InUse`, `NoIoLevel`, `Refused`, `Unsupported` | common errors |

## `ww_spi`

0.1 is an empty trait. `Spi`: `transfer`, `transaction` (CS held: write, read, transfer, delay ops), `write`,
`capabilities`, `configure(SpiConfig) -> SpiActual` (role master / slave, frequency, mode 0-3, bit order, word bits,
CS auto / manual / none), `set_cs`, `reference_voltage: Volt`. `SpiMonitor`: a `frames` stream, one item per CS
assertion with start and end time, `mosi` and `miso` bytes. Slave data paths come later.

## `ww_adc`

**Written** (`ww_stdlib/ww_adc/src/lib.rs`). Three traits: `Adc` (channels with their `ww_si` unit and range, one
calibrated reading), `AdcStream` (continuous raw sampling: `configure_stream`, sideband `Open` / `Close`),
`AdcCapture` (lossless triggered block capture into device RAM, read out after; a one-set capture is the
simultaneous read). A thermometer implements `Adc` only; a data logger adds `AdcStream`; a scope-like device adds
`AdcCapture`.

## `ww_pwm`

`Pwm` with `channel[]: PwmChannel` and `capabilities()` naming the group of each channel (channels of a group share
the frequency) and which channels can capture. `PwmChannel`: `set_output(PwmOutput { freq: Hertz, duty: u16,
inverted, phase_correct }) -> PwmActual`, `set_duty`, `stop(idle: Level)`. `PwmCapture` (`set_capture(mode, gate)`,
`read_capture`, a `capture` stream) for devices that measure. Error `SharedTimer`: the other channel of the group
runs at another frequency.

## `ww_onewire`

`OneWire` with `transaction { rom: Option<RomId>, write, read_len, strong_pullup_ms }`, `reset`, paged `search`,
bit-level `triplet`, `configure { pin, speed: Standard | Overdrive }`, `capabilities`.

## `ww_timebase`

Only the `Clock` trait, no time type (D6): `now() -> Second` (full width, for unwrapping and host ↔ device
correlation), `info()` (resolution, source, sync formats), `configure_sync(Off | Master { format } | Slave
{ format })`, `sync_status()` (`Free`, `Acquiring`, `Locked`, `Holdover`, offset, last lock), a `time_map` stream of
`(local: Second, shared: Second, state)` points about once per second while synced and on every state change, so a
timeline shows unsynced spans, and `set_time_of_day(ww_date_time::DateTime)` for a master.

## `ww_logic_analyzer` (new)

Timestamped capture of many digital channels, for anything from an MCU with a PIO or a DMA-able port to a dedicated
analyzer:

- `LogicAnalyzer`: `capabilities()` (channels with names, max sample rate per channel count, buffer depth,
  encodings, triggers), `configure(LaConfig { channels, rate: Hertz, depth, trigger, pre_trigger })` returning the
  real rate and depth, `arm()`, `status()`, `read(offset, len, encoding)`. Lossless, at any rate the hardware can
  sample into RAM.
- `LogicStream`: continuous capture streamed while the link keeps up (reliability from `ww-stream-reliability`).

Encodings (the reason this is a crate): `Raw` (samples bit-packed), `Transitions` (only changes: a `DeltaOfDelta` of
sample indices plus the new levels, SW-31; a quiet bus costs almost nothing). Triggers: level, edge, pattern,
external. Protocol decoding stays on the host. Design: [logic analyzer and signal generator](logic_analyzer.md).

## `ww_signal_generator` (new)

The reverse: `PatternGenerator` (digital: channels, rate, repeat, a pattern loaded through a `#[credit] sink!` in the
analyzer's `Raw` encoding, so a capture can be replayed; driving several pins at the same instant) and
`WaveformGenerator` (a DAC: sine, square, triangle, sawtooth, DC, arbitrary tables; `freq: Hertz`, `amplitude:
Volt`, `offset: Volt`). Design: [logic analyzer and signal generator](logic_analyzer.md).

## `ww_si` and `ww_numeric` improvements

1. **Everything a cargo feature**: number types already are; quantity groups (electrical, time, mechanical, ...) and
   the host helpers become features too, so a firmware pays only for what it uses.
2. **Time semantics of `Second` as a timestamp**: "device time since its clock's start", the wrap rule for
   fixed-width storage, host helpers (`to_f64_seconds()`, an `Unwrapper` with `now()` as anchor).
3. **Constructors and conversions**: `Volt::millivolts(3300u16)`, `Volt::volts_f32(3.3)`, `to_f64()`, comparisons
   across prefixes and storage; the same for `Hertz`, `Second`, `Ampere`.
4. **Derives on every quantity**: done on this branch (owned, `defmt`, `Debug`; `ww_adc` needed them).
5. **Variable-length storage**: `UVlq32` / `UVlq64` in `NumericValue` (it is `self_describing`, so add them before
   release).
6. **Later, zero-overhead typed quantities** for hot paths: `Volt<u16, Milli>` (storage and prefix in the type, only
   the number on the wire, the unit in the API model), special-cased in codegen like `MaxLen`.

## Versions, compatibility, what breaks

None of `ww_gpio`, `ww_uart`, `ww_i2c`, `ww_spi`, `ww_si` has been published, so they keep 0.1.0 (AGENTS.md: never
published crates are not bumped) and their API snapshots are re-saved; new crates start at 0.1.0. On the wire against
today's 0.1 code: `ww_gpio` sends the same bytes (new `Error` variants are new to old readers); `ww_uart` changes
`tx` and moves `tx_mon` (affects `examples/uart*`); `ww_i2c` changes addresses, levels, bounds, timestamps (no user in
this repo); `ww_spi` gets content. Outside this repo `usb_io_fw` moves to the family in its 0.4.0 (FW-1). No deployed
device besides rePort rev A boards in-house uses these crates.

## Implementation plan

1. **`ww_si` / `ww_numeric` improvements** (items 1-3 and 5), first, since every crate uses them.
2. **Skeleton** in this session: the remaining crates with their traits and types compiling `no_std` and `std`,
   workspace wiring, FEATURES items.
3. **Fill-ins on sonnet, in parallel**, one per crate: doc comments, host helpers, the test pair `tests/io_<x>` with
   a mock server, a docs page, snapshot, CHANGELOG, FEATURES. `ww_gpio` also moves `ww_gpio_hl` and the examples;
   `ww_uart` the uart example. `ww_logic_analyzer` and `ww_signal_generator` get a design pass on opus first.
4. **`cnt` in `examples_mcu`**: counters in the MCU examples and test firmwares, `cnt run` as the cargo runner.
5. When `ww-errors-plan` and `ww-stream-reliability` land: apply them to every crate of the family in one pass.
6. **Review** of the whole family by Roman, then `tpm land`.

## Ideas not asked for

- **Mocks as a reusable simulator** (`ww_io_sim`, std): the test servers wired as a loopback (TX↔RX, an I2C EEPROM
  at 0x50, a 1-wire DS18B20); WW-4 runs crates.io embedded-hal drivers against it, rePort's sim reuses it.
- **`ww` CLI subcommands**: `ww gpio`, `ww uart term`, `ww i2c scan`, `ww adc read`, `ww la capture` on any device
  that has the trait, found by global trait path (`DynClient::traits_only(..)`, `DynClient::global_attachment(gid,
  "Trait")`).
- **`ww record` into rerun**: every stream of one or more devices on rerun timelines (device time, shared time), with
  gaps and unsynced spans visible.
- **sigrok export** of `ww_logic_analyzer` captures and of monitor streams.
