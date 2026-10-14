# IO trait family (design)

!!! warning "Design for review, round 2"
    Draft of 14 Oct 2026 (STD-13 io-trait-family). Round 1 was reviewed by Roman on 14 Oct; his points and the
    answers are in [Review answers](#review-answers-round-1). Nothing is built yet except `ww_i2c` and `ww_adc`,
    written as real crates so their API and doc comments can be read. Once approved,
    each crate is filled in by its own session, and this page becomes the family's overview.

## Scope and principles

The family is for **every device** that exposes IO over WireWeaver: a Nucleo board bridging its pins to a PC, a USB
adapter like an FTDI or Bus Pirate, a motor controller, an ESP32 sensor node, a lab instrument, rePort. rePort is the
first user and the source of the requirements, not the shape of the traits.

1. **Pay only for what you use.** A trait a device doesn't put in its tree costs it nothing; a method it has to
   implement but never supports still costs a dispatch entry and a stub. So the family is many small traits: a core
   trait per bus that every implementer can fill in, and separate traits for monitors, streams, captures, bus
   recovery. Bigger subjects get their own crates: `ww_logic_analyzer`, `ww_signal_generator`.
2. **Evolve what works.** `ww_gpio` (usable, used), `ww_uart` and `ww_i2c` (prototypes) keep every resource, index,
   name and type that works; each section lists what is kept, changed (and why) and added.
3. **Typed values through `ww_si`.** Voltages, frequencies and timestamps are `ww_si` quantities, not `u16` millivolts
   or a new tick type. The device reports in whatever storage is convenient for it (a `u32` of µs, an `f32` of V);
   the host reads them all ([ww_si improvements](#ww_si-and-ww_numeric-improvements)).
4. **Ergonomic first, fast paths separate.** `ww_gpio` stays low-speed, logical-pin GPIO. Sampling many pins fast
   is a logic analyzer, driving many pins fast is a signal generator. Many small calls in one round trip are
   multi-call (CLT-15), a framework feature, not a method per trait.
5. **Capabilities are queried**, never assumed: every core trait has `capabilities()`.

## Crates and traits

| Crate | Traits | Status |
|---|---|---|
| `ww_gpio` | `Bank`, `Pin` | evolves 0.1, additive except errors |
| `ww_uart` | `Uart`, `UartMonitor` | evolves 0.1 |
| `ww_i2c` | `I2c`, `I2cBus`, `I2cMonitor` | evolves 0.1, **written, read the rustdoc** |
| `ww_spi` | `Spi`, `SpiMonitor` | new content (0.1 is empty) |
| `ww_adc` | `Adc`, `AdcStream`, `AdcCapture` | new, **written, read the rustdoc** |
| `ww_pwm` | `Pwm` with `channel[]: PwmChannel`, `PwmCapture` | new |
| `ww_onewire` | `OneWire` | new |
| `ww_timebase` | `Clock` | new: device time and sync, no new time type |
| `ww_logic_analyzer` | `LogicAnalyzer`, `LogicStream` | new (round 2) |
| `ww_signal_generator` | `PatternGenerator`, `WaveformGenerator` | new (round 2) |
| `ww_si` / `ww_numeric` | — | improved for the above |

A device picks traits, for example a Nucleo bridge: `gpio: ww_gpio::Bank`, `uart: ww_uart::Uart`,
`i2c: ww_i2c::I2c`; rePort adds `uart_monitor: ww_uart::UartMonitor`, `i2c_bus: ww_i2c::I2cBus`,
`i2c_monitor: ww_i2c::I2cMonitor`, `la: ww_logic_analyzer::LogicAnalyzer`, and so on. Each trait sits at most once in
a device's tree and outside trait arrays, so generic tools find it by global trait path (API-20, CLT-10).

## Review answers (round 1)

Roman's points, condensed, each with the answer and, where he asked "why" or "not clear", a scenario.

**General: don't design narrowly for rePort; no bloat; split traits; own crates for logic analysis and signal
generation.** Done: see [Scope and principles](#scope-and-principles) and [Crates and traits](#crates-and-traits).
Monitors, ADC streams and captures, I2C bus control are separate traits now; `ww_logic_analyzer` and
`ww_signal_generator` are new crates; `read_inputs` / `write_outputs`, `TimedPinEvent` and `read_many` are gone.

**General: use `cnt` from the start; `cargo run` through `cnt run`.** Yes: a task for `examples_mcu/*` and the test
firmwares (counters in the MCU examples, `runner = "cnt run --chip ..."` in each board's `.cargo/config.toml`, as
`usb_io_fw` does), filed with the fill-ins. Mock servers that run on an MCU count dropped stream data with `cnt!`
too, so `lost` in a stream item can be checked against the counter.

**D1 common errors: for whom? Why limit the error type? Per-method errors?**

- *For whom:* every IO crate of `ww_stdlib`, for every device. Three situations come back on all kinds of hardware,
  and a generic tool should show them the same way:
    - `InUse`: a Nucleo bridge is asked to make PA2 a GPIO output while USART2 uses it; an adapter is asked for SPI
      while its pins run I2C.
    - `NoIoLevel`: an adapter with level translators and a VIO pin (Bus Pirate, rePort, most "universal" adapters)
      has no IO voltage, so no output can work.
    - `Refused`: a safety rule says no: a motor controller refuses to drive a half-bridge output while the other
      side is on, rePort refuses 3.3 V I2C on a bus that sits at 5 V.
- *Limits:* none. An error is a normal `shrink_wrap` value, so variants carry data where it helps:
  `Refused { reason: MaxLen<&'i str, 64> }`, `InUse { by: MaxLen<&'i str, 32> }` ("USART2"), `Nack { at: u16 }`.
  A device that has nothing to say sends an empty string (one byte). Round 1 kept them bare only to save flash,
  which is the device's choice, not the type's.
- *Per method:* Rust has not changed: there are still no anonymous error sets (`terrors` and `error_set` are macro
  crates, not language features, and don't fit generated code). WireWeaver could do better at the API level later,
  since it knows each method: a method could declare which variants it returns and introspection could show it
  ([ideas](#ideas-not-asked-for)). For now, **one error enum per crate**, and every method's rustdoc has an
  `# Errors` section listing what it can return (see the `ww_i2c` rustdoc).

**D2 IO levels: `u16` millivolts won't do; improve `ww_si`.** Agreed: levels are `ww_si::Volt` everywhere,
`ww_i2c` changes `io_level_mv: u16` to `io_level: Volt`. `Volt` already lets the device pick the storage (`U16` of
mV, `F32` of V; negative and sub-mV values work); it costs a prefix and a type tag, one byte together, irrelevant for
configuration calls. For hot paths a zero-overhead typed form is proposed in
[ww_si improvements](#ww_si-and-ww_numeric-improvements).

**D3 methods returning the actual value set.** Agreed, unchanged: setters that round (baud, frequency) are methods
returning what was set; plain choices stay properties.

**D4 `seq` in which context, what are "units"?** Per stream, counted from when the stream started (`configure`,
`start`), wrapping `u32`. The unit is what the stream carries, said in its doc: bytes for UART, transactions for the
I2C monitor, frames for the SPI monitor, sample sets for the ADC. Scenario: a UART stream sends three chunks of 10,
5 and 20 bytes: their `seq` are 0, 10, 15; the next one has `seq` 35. Counting units, not items, makes the position
of every byte exact: a protocol decoder on the host can say "frame starts at byte 4711 of this capture", and two
recordings of the same stream line up.

**D5 the `lost` field: what is it about?** It is how a device says "I had to throw data away here". A stream
producer (an interrupt, DMA) can't wait for USB; when the host is slow (a Python notebook busy for a second) the
device's buffer fills up and newer data is dropped. Scenario without `lost`: a UART monitor runs at 2 Mbaud, the
host stalls for 300 ms, the device drops 60 000 bytes and then carries on. The host glues the chunk before the gap
to the one after it, the protocol decoder shows a corrupt frame, and the user hunts a bug in the device under test
that doesn't exist. With `lost`: the next chunk says `lost = 60000` (and its `seq` jumped by as much), the host shows
"60 000 bytes lost at 12.345 s", and the decoder resynchronises after the gap. It is one field in every stream item,
4 bits when nothing was lost.

**D6 timestamps: use `ww_si`, no new type; is `UVlq64` right?** Agreed, `Ticks` is gone. A timestamp is a
`ww_si::Second`: device time since its clock started (boot, or the `Clock`'s epoch), in the prefix and storage the
device picks. `UVlq64` was the wrong default: its size keeps growing with uptime. The family's convention instead:

- the device picks the storage its timer gives cheaply: `U32` of µs on most MCUs (5 bytes with prefix and tag), `U64`
  where it has a 64-bit counter, `UNib32` of ms for slow sensors;
- a fixed-width value **wraps** (a `U32` of µs every 71.6 minutes) and the host unwraps it: it sees items far more
  often than that, and `Clock::now()` gives it the full value whenever it asks;
- every stream item and capture of one device uses the same clock, so items of different streams line up.

This needs no new type, only the wrap rule and helpers in `ww_si` ([below](#ww_si-and-ww_numeric-improvements)).

**D7 more context; the clock doesn't stop without sync.** Right, and the design agrees: every device has one
free-running clock that timestamps everything; sync only adds a mapping to a shared time. Round 1's D7 was about a
device that has no clock at all (a tiny MCU without a free timer): its items carry `t` = 0, and `capabilities()`
says timestamps are not real. When a synced device loses its sync, `Clock::sync_status` and the `time_map` stream say
so (`Holdover`, then `Free`), each `time_map` point carries the state, and a timeline renders that span as "not
synced" (e.g. hatched), with timestamps still in local time.

**D8 why future versions, we can break the wire before release?** Before anything reaches people, yes, we break
freely. `#[future_version]` is for after the first release. Scenario: rePort 1.0 ships; a lab automates tests with a
Python script on the 1.0 host library and never updates it. Firmware 1.0.5 adds `I2cError::BusStuck` (an additive
change, so a patch). Without `Unknown(u32)`, the old script gets an I2C error reply it can't decode at all and fails
with "malformed data", even though it would have handled any error fine. With it, the script gets
`I2cError::Unknown(9)` and its error path runs. SW-16 has landed (opt-in), so it is one line per growing enum.

**D9 bulk `u32` pin masks: not ergonomic, non-consecutive indices, 33 pins, a rePort crutch.** Agreed, removed.
`ww_gpio` addresses pins by their logical index only. Reading eight pins in one round trip is multi-call (CLT-15),
generic for every trait; sampling or driving many pins at a precise time is a logic analyzer or a signal generator.

**D10 one test crate per trait.** Yes: `tests/io_<trait>` + `tests/io_<trait>_api`, each with a mock server that
doubles as a simulator for host crates (WW-4).

**`TimedPinEvent` is a logic analyzer built out of GPIO.** Agreed: removed. `ww_gpio` keeps its 0.1 `event` stream
for low-speed edges (a button, an alert line), and `ww_logic_analyzer` does timestamped capture of many channels
properly, with bit packing and compression ([below](#ww_logic_analyzer-new)); `ww_signal_generator` is its reverse.

**`ww_uart`, `ww_spi`, `ww_pwm`, `ww_onewire` look right, try and see.** Kept as in round 1, with the round 2 rules
applied (monitors and capture split out, `ww_si` types).

**`ww_i2c` and `ww_adc` unclear: render them with proper doc comments.** Written as real crates with full doc
comments on this branch; they build `no_std` and `std`, pass clippy and rustdoc with `-D warnings`, and their API
snapshots are saved. One finding: **rustdoc can't show a `#[ww_trait]` at all** (the attribute expands to nothing),
so `cargo doc` shows only the types of a stdlib crate, never its methods. For this review the sources are rendered
as HTML pages instead; the real fix is a framework item ([ideas](#ideas-not-asked-for)): `#[ww_trait]` emitting a
doc-only Rust trait under `cfg(doc)`, so every stdlib crate's methods show up on docs.rs.

**`ww_adc::read_many`: why, with multi-call coming?** Removed. Its one real reason was simultaneous sampling (three
phase currents taken at the same instant), which multi-call can't give; that is a one-set capture in `AdcCapture`
now.

**Mocks and test instrumentation, `ww` subcommands, sigrok export, rerun.** Mocks: yes. Subcommands: yes. Sigrok
export: yes (for `ww_logic_analyzer` captures first). Rerun: a good fit for the timeline idea: `ww record` writes
every stream of one or more devices into a rerun recording, with `seq` / `lost` / `t` mapped to rerun's timelines,
gaps and unsynced spans visible; added to [ideas](#ideas-not-asked-for).

## Conventions for every IO trait

These hold for all crates of the family and for IO traits added later; `docs/api/streams.md` gets the stream part.

**Types.** New types are `Unsized` unless they are bit-level leaves, `owned(feature = "std")`,
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`. Every list, byte buffer and string is bounded with
`MaxLen` (raising a bound later is compatible, lowering is not). Physical values are `ww_si` quantities.

**Enums that can grow** (errors, modes, record kinds, triggers, units) are `Unsized` with `ww_repr = unib32` and end
with `#[future_version] Unknown(u32)` (SW-16 is opt-in, so it is added explicitly). Enums complete by nature (`Level`,
`SpiMode`, `BitOrder`) don't get it. The existing `self_describing` 0.1 enums (`Mode`, `Pull`, `Speed`, `I2cMode`)
stay as they are: SW-16 lets them survive new unit variants only.

**Configuration.** Plain choices are properties. Settings the hardware rounds are methods returning what was set
(`i2c_configure` returns the real frequency, `Uart::set_baud` the real baud). A trait with many coupled settings
takes one config struct (new fields with `#[default = ..]` only); `Disabled` is a mode, disabling releases the pins.

**Errors.** One enum per crate. Every IO crate has these variants by these names (appended after the 0.1 variants
of existing crates, first in new ones):

```rust
    /// A pin or peripheral this needs is held by another function
    InUse { by: MaxLen<&'i str, 32> },
    /// The IO level translators have no supply
    NoIoLevel,
    /// A safety rule of the device doesn't allow it
    Refused { reason: MaxLen<&'i str, 64> },
    /// A variant from a newer version of this API (SW-16)
    #[future_version]
    Unknown(u32),
```

Each method's rustdoc lists the variants it returns in an `# Errors` section.

**Streams carry a head**, the same three fields by name, so a generic recorder handles every stream:

| Field | Type | Meaning |
|---|---|---|
| `seq` | `u32` | Position of the item's first unit since the stream started, wrapping. The unit is said in the stream's doc |
| `lost` | `UNib32` | Units dropped right before this item; `seq` counts them too |
| `t` | `ww_si::Second` | Device time of the item's first event, see D6 |

Then the item's own fields, the payload last. Producers never wait: when the buffer is full the newest data is
dropped and counted (`cnt!` on the device, `lost` in the next item). Opening a stream starts nothing; a configure or
start method does. Host → device bulk data goes through `#[credit] sink!` (API-28).

**Resource order** is wire-visible: the most used resources first (indices 0-7 encode in 4 bits).

## `ww_gpio`

0.1 is usable and used (`ww_gpio_hl`, `examples/all_gpio`, three `examples_mcu` boards). The new version is 0.1 plus a
per-pin `capabilities()` and the common errors; everything 0.1 sends keeps its bytes.

| | Item | Why |
|---|---|---|
| kept | `Bank`: `pin[]`, `reserved!(1..=7)`, `capabilities()`, `reference_voltage: Volt`, `name()` | works |
| kept | `Pin`: `set_output_level` 0, `toggle` 1, `output_level` 2, `input_level` 3, `event` 7, `set_mode`, `mode`, `pull`, `speed`, `configure_events` | works, indices are wire |
| kept | `Level`, `Mode`, `Pull`, `Speed`, `IoPinEvent`, `IoPinEnabledEvents`, `BankCapabilities`, all `Error` variants | works |
| added | `Pin::capabilities() -> PinCapabilities` (appended): modes, pulls, speeds, `name`, whether events work | which pin can do what differs per pin on most MCUs and adapters |
| added | `Error`: `InUse`, `NoIoLevel`, `Refused`, `Unknown` (appended) | common errors |
| removed from round 1 | `read_inputs`, `write_outputs`, `timed_event` | D9, logic analyzer |

## `ww_uart`

0.1 is a prototype used by `examples/uart`. The new version keeps the trait shape and its properties, changes the TX sink and
the RX item, and moves monitoring into its own trait.

| | Item | Why |
|---|---|---|
| kept | `rx` stream at 0, `capabilities()`, properties `baud_rate`, `mode`, `stop_bits`, `parity`, `prevent_back_feed`, `reference_voltage`, `set_pin_level` | work |
| kept | `BaudRate`, `Mode`, `Pin`, `StopBits`, `Parity`, `Capabilities` fields, `Error` variants | work |
| changed | `sink!(tx: [u8])` → `#[credit] sink!(tx: MaxLen<RefVec<'i, u8>, 256>)` | a plain sink overruns the device's TX buffer on bulk writes (a file sent to the UART); credit sinks (API-28) exist for this. Wire break |
| changed | `RxChunk { flags, timestamp: Option<Second>, bytes }` → `RxChunk { seq, lost, t: Second, flags, bytes }` | the stream head; `seq` counts bytes. Wire break of the item |
| moved | `tx_mon` → `UartMonitor` trait: `line_a` / `line_b` streams of `RxChunk`, `configure_monitor` | a device without a second receive line doesn't carry the stream and its stubs |
| added | `data_bits` property, `set_baud(BaudRate) -> Result<u32, Error>` returning the real baud, `send_break`, `tx_pending` | missing settings; D3 |
| added | `RxFlags`: `framing_error`, `overrun`, `break_detected` (appended with defaults) | error flags |
| added | `Error`: common variants, `Timeout` | |

## `ww_i2c`

**Written; read the rustdoc** (`ww_stdlib/ww_i2c/src/lib.rs`). Summary of what changes against 0.1:

| | Item | Why |
|---|---|---|
| kept | `I2c`: `i2c_capabilities`, `i2c_configure`, `i2c_write`, `i2c_read` with `I2cReadKind`, `i2c_scan`, `delay_after`, names and order; `u8` 7-bit addresses; `I2cMode`; `I2cError` variants | work |
| changed | `io_level_mv: u16` → `io_level: Volt` | D2 |
| changed | `Vec<u8>` → `MaxLen<RefVec<'i, u8>, N>` | a `no_alloc` device server can't take a `Vec`; bounded sizes |
| changed | `RawTimestamp { ticks: u32 }` → `ww_si::Second` | D6 |
| changed | `delay_after: u32` (no unit) → `Option<Second>` | typed; 1 bit when there is no delay instead of 4 bytes |
| changed | plain structs get `#[derive_shrink_wrap]`; `I2cError` becomes `Unsized` | they cross the wire; errors carry data now |
| added | `i2c_transaction` | repeated starts, embedded-hal `transaction` |
| added | `I2cBus` trait: lines state, clock wiggle (recovery), pull-ups | only adapters with that hardware implement it |
| added | `I2cMonitor` trait: one record per transaction | only sniffers implement it |

## `ww_spi`

0.1 is an empty trait. `Spi`: `transfer`, `transaction` (CS held: write, read, transfer, delay ops), `write`,
`capabilities`, `configure(SpiConfig) -> SpiActual` (role master / slave, frequency, mode 0-3, bit order, word bits,
CS auto / manual / none), `set_cs`, `reference_voltage: Volt`. `SpiMonitor`: a `frame` stream, one item per CS
assertion with `mosi` and `miso` bytes, `seq` counting frames. Slave data paths come later as `SpiTarget`.

## `ww_adc` (new)

**Written; read the rustdoc** (`ww_stdlib/ww_adc/src/lib.rs`). Three traits: `Adc` (channels with their unit, one
calibrated reading), `AdcStream` (continuous sampling, raw samples, the stream head), `AdcCapture` (lossless block
capture into device RAM, triggered, read out after; a one-set capture is the simultaneous read). A thermometer
implements `Adc` only; a data logger adds `AdcStream`; a scope-like device adds `AdcCapture`.

## `ww_pwm` (new)

As round 1: `Pwm` with `channel[]: PwmChannel` and `capabilities()` naming the group of each channel (channels of a
group share the frequency) and which channels can capture. `PwmChannel`: `set_output(PwmOutput { freq: Hertz,
duty: u16, inverted, phase_correct }) -> PwmActual`, `set_duty`, `stop(idle: Level)`. Capture moves to its own
trait, `PwmCapture` (`set_capture(mode, gate)`, `read_capture`, a `capture` stream), so output-only devices don't
carry it. Error `SharedTimer`: the other channel of the group runs at another frequency.

## `ww_onewire` (new)

As round 1: `OneWire` with `transaction { rom: Option<RomId>, write, read_len, strong_pullup_ms }`, `reset`,
paged `search`, bit-level `triplet`, `configure { pin, speed: Standard | Overdrive }`, `capabilities`.

## `ww_timebase` (new)

Only the `Clock` trait, no time type (D6): `now() -> Second` (full width, for unwrapping and host ↔ device
correlation), `info()` (resolution, source, sync formats), `configure_sync(Off | Master { format } | Slave
{ format })`, `sync_status()` (`Free`, `Acquiring`, `Locked`, `Holdover`, offset, last lock), a `time_map` stream of
`(local: Second, shared: Second, state)` points about once per second while synced and on every state change (D7),
and `set_time_of_day(ww_date_time::DateTime)` for a master.

## `ww_logic_analyzer` (new)

Timestamped capture of many digital channels, for anything from an MCU with a PIO or a DMA-able port to a dedicated
analyzer. Two traits:

- `LogicAnalyzer`: `capabilities()` (channels with names, max sample rate per channel count, buffer depth,
  encodings, triggers), `configure(LaConfig { channels: MaxLen<RefVec<u8>, 32>, rate: Hertz, depth: u32, trigger,
  pre_trigger: u32 })` returning the real rate and depth, `arm()`, `status()` (armed, triggered at, done),
  `read(offset, len, encoding)` returning a block. Lossless, at any rate the hardware can sample into RAM.
- `LogicStream`: continuous capture streamed to the host while it can keep up, with the stream head (`seq` counts
  samples) and drops reported; the rate limit is the link.

Encodings (the reason this is a crate): `Raw` (samples bit-packed, `n` channels per sample), `Transitions` (only
changes: a `DeltaOfDelta` of sample indices plus the new levels, SW-31; a quiet bus costs almost nothing), maybe
`Rle`. The host turns either into the same timeline, sigrok export, rerun. Triggers: level, edge on a channel,
pattern, `Sync` (another device's trigger). Protocol decoding stays on the host.

## `ww_signal_generator` (new)

The reverse. Two traits, a device implements what its hardware does:

- `PatternGenerator` (digital): `configure(channels, rate: Hertz, repeat: Once | Loop | Count(u32))`, a pattern
  loaded through a `#[credit] sink!` of bit-packed samples (the analyzer's `Raw` encoding, so a capture can be
  replayed), `start`, `stop`, `status`. Driving several pins at the same instant is this (the D9 use case done
  properly).
- `WaveformGenerator` (analog, a DAC): `set_waveform(Sine | Square | Triangle | Sawtooth | Dc | Arbitrary,
  freq: Hertz, amplitude: Volt, offset: Volt)`, arbitrary tables through a credit sink, `start`, `stop`.

## `ww_si` and `ww_numeric` improvements

What the family needs from them (filed as one piece, before the crates that use them):

1. **Time semantics of `Second` as a timestamp:** documented "device time since its clock's start", the wrap rule
   for fixed-width storage, and host helpers: `Second::to_f64_seconds()`, an `Unwrapper` that turns a sequence of
   wrapping timestamps into monotonic ones (with `now()` as an anchor).
2. **Constructors and conversions** for the common cases: `Volt::millivolts(3300u16)`, `Volt::volts_f32(3.3)`,
   `to_f64()`, comparisons across prefixes and storage; the same for `Hertz`, `Second`, `Ampere`.
3. **Derives on every quantity**: `owned(feature = "std")` and `defmt` were only on `Volt` and `Second`. Done on
   this branch (`ww_adc` needed `Hertz`, `BaseUnit`, `Prefix`).
4. **Variable-length storage**: `NumericValue` has `UNib32` but no `UVlq32` / `UVlq64`; adding them lets a device
   pick a byte-aligned variable-length timestamp. `NumericValue` is `self_describing`, so new data variants break old
   readers: add them now, before release.
5. **Later, zero-overhead typed quantities** for hot paths: `Volt<u16, Milli>` (storage and prefix in the type,
   only the number on the wire, the unit recorded in the API model), special-cased in codegen like `MaxLen`. A
   framework item, not needed now: the dynamic form costs one byte per value.

## Versions, compatibility, what breaks

None of `ww_gpio`, `ww_uart`, `ww_i2c`, `ww_spi`, `ww_si` has been published, so they keep 0.1.0 (AGENTS.md: never
published crates are not bumped) and their API snapshots are re-saved; new crates start at 0.1.0. What changes on
the wire against today's 0.1 code:

- `ww_gpio`: nothing 0.1 sends changes; new `Error` variants are new to 0.1 readers, exhaustive Rust matches need
  new arms.
- `ww_uart`: `tx`, `RxChunk`, `tx_mon` (moved). Affected: `examples/uart*`.
- `ww_i2c`: level type, bounds, timestamps, errors. No user in this repo.
- `ww_spi`: the empty trait gets content.
- Outside this repo: `usb_io_fw` moves to the family in its 0.4.0 (FW-1). No deployed device besides rePort rev A
  boards in-house uses these crates, so the breaks cost nothing now; after the first publish the evolution rules
  apply.

## Implementation plan

1. **`ww_si` / `ww_numeric` improvements** (items 1-4 above), first, since every crate uses them.
2. **Skeleton** in this session: every crate with its traits and types compiling `no_std` and `std`, workspace
   wiring, stream conventions in `docs/api/streams.md`, FEATURES items. `ww_i2c` and `ww_adc` are already written.
3. **Fill-ins on sonnet, in parallel** (`--from-here`), one per crate: rustdoc, host helpers, the test pair
   `tests/io_<x>` with a mock server, a docs page, snapshot, CHANGELOG, FEATURES. `ww_gpio` also moves
   `ww_gpio_hl` and the examples; `ww_uart` the uart example. `ww_logic_analyzer` and `ww_signal_generator` get their
   own design pass on opus first (encodings).
4. **`cnt` in `examples_mcu`**: counters in the MCU examples and test firmwares, `cnt run` as the cargo runner.
5. **Review** of the whole family by Roman, then `tpm land`.

## Ideas not asked for

- **Mocks as a reusable simulator** (`ww_io_sim`, std): the test servers wired as a loopback (TX↔RX, an I2C EEPROM
  at 0x50, a 1-wire DS18B20); WW-4 runs crates.io embedded-hal drivers against it, rePort's sim reuses it.
- **`ww` CLI subcommands**: `ww gpio`, `ww uart term`, `ww i2c scan`, `ww adc read`, `ww la capture` on any device
  that has the trait, found by global trait path: `DynClient::traits_only(..)` and `DynClient::global_attachment(gid,
  "Trait")` (API-20 / CLT-10).
- **`ww record` into rerun**: every stream of one or more devices on rerun timelines (device time, shared time), with
  gaps from `lost` and unsynced spans from `time_map`.
- **sigrok export** of `ww_logic_analyzer` captures and of monitor streams.
- **`#[ww_trait]` in rustdoc**: under `cfg(doc)` the attribute emits a plain Rust trait with the same methods,
  streams and properties as documented items (streams and properties as methods with a note), so docs.rs shows
  every stdlib trait. Today it shows only the types.
- **Per-method error lists in the API model**: a method declares which variants of its crate's error it returns;
  introspection and generated docs show them, a generated client could narrow the type.
