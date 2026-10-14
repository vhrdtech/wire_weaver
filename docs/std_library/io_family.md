# IO trait family 0.2 (design)

!!! warning "Design for review"
    Draft of 14 Oct 2026 (STD-13 io-trait-family), **waiting for Roman's review**. Nothing here is built yet. It
    turns §4 of the rePort API design (`usb_io_fw` `docs/api_design.md`, approved 14 Oct 2026) into exact traits and
    types, settles what §4 left open, and lists where it deviates from §4 ([Decisions](#decisions-for-review)). Once
    approved, each crate is filled in by its own session, and this page becomes the family's overview.

The family: `ww_timebase` (new), `ww_gpio` 0.2, `ww_uart` 0.2, `ww_i2c` 0.2, `ww_spi` 0.2, `ww_adc` (new), `ww_pwm`
(new), `ww_onewire` (new). `ww_files` is not part of this round (WW-5, v1.0).

**`ww_gpio`, `ww_uart` and `ww_i2c` evolve, they are not redesigned** (Roman, 14 Oct). Their 0.2 keeps every
resource, index, name and type of 0.1 that works, fills `reserved!` slots and appends what is missing, and changes
only what can't work for the family; each section has a table of what is kept, changed (and why) and added. Only
`ww_spi` (an empty stub) and the new crates are designed from scratch, following the family conventions below.

Goal: one device exposing these traits is driven by generic tools (the `ww` CLI, `ww_*_hl` host crates with
embedded-hal, a GUI) without knowing the device, and every addition the rePort roadmap expects (monitors, PIO
capture, per-byte timestamps, RS-485, slave modes) is wire-compatible later.

## Decisions for review

Recommendations first; each is a question for Roman, the rest of the page follows the recommendation.

| # | Question | Recommendation | Deviates from §4? |
|---|---|---|---|
| D1 | Common errors: one shared error type in a common crate, or the same variants in every crate's own enum? | **Same names in every crate's own enum**: `InUse`, `NoIoLevel`, `Refused` appended to the existing error enums (their 0.1 variants keep their positions), first in the new crates. No extra crate in the dependency chain; generic tools match by name through introspection. | refines |
| D2 | IO levels: `ww_si::Volt` (gpio, uart today) or millivolts (i2c today)? | **Keep what each crate has**: `reference_voltage: Volt` property in `ww_gpio` and `ww_uart`, `io_level_mv` argument of `i2c_configure`. `ww_spi` takes the `Volt` property like uart. Unifying would break working code for a cosmetic gain. | no |
| D3 | UART configuration: one `configure(Config)` (§4.2) or the per-setting properties of 0.1? | **Keep the 0.1 properties** (`baud_rate`, `mode`, `parity`, `stop_bits`, ...), append `data_bits` and `baud_rate_actual()`. They work, and a GUI shows one property per setting. Half-configured states are avoided by configuring while `mode` is `HighZ`. New traits (`ww_spi`, `ww_onewire`) use `configure(Config) -> Actual`, like `i2c_configure` returning the actual frequency. | yes: §4.2 had a `Config` struct |
| D4 | What does `seq` count? | **Units of the stream** (bytes, records, frames, sample sets), wrapping `u32`, lost units included. `next.seq == prev.seq + prev.units + next.lost`, so a gap is exact and visible even if one `lost` is missed. | refines |
| D5 | Lost data: a `Lost` record kind / variant (§4.3, §4.4) or a `lost` field? | **`lost: UNib32` in every new stream item**: units dropped right before this item. 4 bits when 0, one rule for every stream, no special records. | yes: drops `RecordKind::Lost`, `FrameFlags::Lost` |
| D6 | Timestamps | **`Ticks(UVlq64)`, absolute, local, in every new stream item** (as §4.5). Delta-coded timestamps inside an item come later as optional appended fields (SW-31). | no |
| D7 | Where the timebase lives on a device without `ww_timebase::Clock` | Items still carry `Ticks`; `capabilities()` says whether timestamps are real (`rx_timestamps` in uart 0.1 already does); without them `t` is 0. A `Clock` anywhere in the tree defines the tick rate (one per device). | refines |
| D8 | Enum growth before SW-16 future-version lands | Every enum of the family expected to grow (not only errors) ends with `#[future_version] Unknown(u32)`, added explicitly since SW-16 is opt-in (SW-16 as planned in WW-3: opt-in, last, no wire number, catches any unknown discriminant), and new enums that will grow are `Unsized` with `ww_repr = unib32`. The family ships only **after SW-16**, so an old host survives a new error variant. Existing `sized` / `self_describing` enums (`Level`, `Mode`, `Pull`, `I2cMode`, ...) stay as they are: they can still get unit variants. | no (§11.3) |
| D9 | `read_inputs` / `write_outputs` width | `u32` (32 pins per bank), as §4.1. Wider banks are split into banks. | no |
| D10 | Test layout | One test crate pair per trait (`tests/io_gpio` + `tests/io_gpio_api`, ...): parallel fill-ins don't touch each other's files; each mock server is a small loopback simulator that WW-4 (`_hl` crates) reuses. | n/a |
| D11 | UART `RxChunk`: break it for the stream head (`seq`, `lost`, `t: Ticks` first), or append the head after `bytes` and leave `timestamp: Option<Second>` unused? | **Break it**: `ww_uart` 0.2 breaks anyway (the TX credit sink), the item is read by every UART tool, and a dead `timestamp` field next to `t` would confuse every one of them. | no |
| D12 | I2C addresses: keep `u8` (7-bit) or an `Address { Seven(u7), Ten(u10) }` enum (§4.3)? | **Keep `u8`**, 7-bit; above `0x7F` is `MalformedRequest` (exists in 0.1). 10-bit devices are rare, and the enum would cost every call a discriminant and every caller a change. 10-bit can come later as appended `i2c_*10` methods. | yes: §4.3 had `Address` |

Not decided here, left to the rePort firmware: board-specific rules behind `Refused` and `InUse` (§5), channel lists.

## Conventions for every IO trait

These hold for all crates of the family and for IO traits added later; `docs/api/streams.md` gets the stream part.
Existing crates follow them where 0.1 doesn't already decide otherwise.

**Types.** Every new type is `Unsized` unless it is a bit-level leaf (`Edge`), `owned(feature = "std")`, `borrowed`
where it has no lifetime, `cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`. Every list, byte buffer and
string is bounded with `MaxLen` (raising a bound later is compatible, lowering is not). New enums that can grow:
`ww_repr = unib32`, not `self_describing`/`sized`. **SW-16 is opt-in**: every enum expected to grow (errors, modes,
record kinds, capture modes, triggers, units, sync formats; new and existing ones) gets
`#[future_version] Unknown(u32)` as its last variant explicitly, before the crate is released; one without it fails
old readers on a new variant. Leaf enums that are complete by nature (`Level`, `Edge`, `SpiMode`, `BitOrder`) don't.
Whether the existing `sized` / `self_describing` 0.1 enums (`Mode`, `Pull`, `Speed`, `I2cMode`) can take it is
checked in the skeleton; if not, they stay closed and grow through new resources. Pins and channels are indices, never MCU pin names.

**Configuration** of a new trait is one struct argument, `configure(Config) -> Result<Actual, Error>`, returning what
was really set (rounded baud and frequency). `config() -> Actual` reads it back. A config struct gets new fields with
`#[default = ..]` only. `Disabled` is a mode of the config, not a separate method: disabling releases the pins.
Existing traits keep their shape (D3).

**Capabilities** are queried, never assumed: every trait has a `capabilities()` saying the ranges, which modes
exist, whether timestamps are real, buffer sizes and each stream's maximum rate alone.

**Errors.** Each crate keeps (or gets) one error enum. These variants, by these names, are in every one of them:

```rust
    /// A pin or peripheral this needs is held by another function (the device API says which)
    InUse,
    /// The translators have no level (IO level off)
    NoIoLevel,
    /// Board-safety rules don't allow it (the device API says why)
    Refused,
    /// A variant from a newer version of this API (SW-16), always last, takes no wire number
    #[future_version]
    Unknown(u32),
```

Existing enums append them after their 0.1 variants. New crates start with `Unsupported`, `InUse`, `NoIoLevel`,
`Refused`, `InvalidArgument`, `WrongMode`, then their own. Errors stay short and typed (§14 Q14); details come from the
device-specific API (`frontend.state()` on rePort).

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

`ww_gpio` 0.1 is usable and used (`ww_gpio_hl`, `examples/all_gpio`, three `examples_mcu` boards). 0.2 is 0.1 plus
additions; everything 0.1 sends keeps its bytes. It is a 0.2 and not a 0.1.1 only because `Error` gets variants (a
Rust break for exhaustive matches, and 0.1 readers can't decode them).

| | Item | Why |
|---|---|---|
| kept | `Bank`: `pin[]` at 0, `capabilities()`, `reference_voltage: Volt`, `name()` | works |
| kept | `Pin`: `set_output_level` 0, `toggle` 1, `output_level` 2, `input_level` 3, `reserved!(4..=6)`, `event` 7, `set_mode`, `mode`, `pull`, `speed`, `configure_events` | works, indices are wire |
| kept | `Level`, `Mode`, `Pull`, `Speed`, `IoPinEvent`, `IoPinEnabledEvents`, `BankCapabilities`, all `Error` variants | works |
| added | `Bank::read_inputs() -> u32` in reserved slot 1, `Bank::write_outputs(mask, levels)` in slot 2 | §4.1: parallel buses from the host; low indices for a hot path |
| added | `Pin::capabilities() -> PinCapabilities` (appended) | §4.1: per-pin modes, direction switch time (rePort IO0-3 vs IO4-7) |
| added | `Pin::timed_event: TimedPinEvent` stream (appended) | §4.1: edges with `seq` / `lost` / `t`; `event` stays for devices without a timebase |
| added | `IoPinEnabledEvents`: `#[default] timed: bool`, `#[default] debounce_us: u32` (appended) | `timed` sends edges on `timed_event` instead of `event`; debounce was a `TODO` |
| added | `BankCapabilities`: `#[default] atomic_write_outputs: bool` (appended) | says whether `write_outputs` is one register write |
| added | `Error`: `InUse`, `NoIoLevel`, `Refused`, `#[future_version] Unknown(u32)` (appended) | common errors (D1, D8) |
| changed | — | nothing |

```rust
#[ww_trait]
trait Bank {
    ww_impl!(pin[]: Pin);                                      // 0  kept
    /// Input levels of all pins at once, bit n = pin index n (pins of an array without index n read 0)
    fn read_inputs() -> u32;                                   // 1  new, was reserved
    /// Sets the outputs of the pins in `mask` at once, where the hardware can (`atomic_write_outputs`)
    fn write_outputs(mask: u32, levels: u32) -> Result<(), Error>; // 2  new, was reserved
    reserved!(3..=7);
    fn capabilities() -> BankCapabilities<'i>;                 // kept
    property!(rw reference_voltage: Volt, Error);              // kept
    fn name() -> &'i str;                                      // kept
}

#[ww_trait]
pub trait Pin {
    // 0..=7 and set_mode .. configure_events: kept as in 0.1
    /// What this pin can do
    fn capabilities() -> PinCapabilities<'i>;                  // new
    /// Edges with their time, if enabled with `IoPinEnabledEvents::timed`
    stream!(timed_event: TimedPinEvent);                       // new
}

pub struct TimedPinEvent { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub event: IoPinEvent }
pub struct PinCapabilities<'i> {
    pub modes: MaxLen<RefVec<'i, Mode>, 8>, pub pulls: MaxLen<RefVec<'i, Pull>, 4>,
    pub speeds: MaxLen<RefVec<'i, Speed>, 4>,
    pub dir_switch_ns: u32,                  // how long a direction change takes (open drain emulation)
    pub events: bool, pub timed_events: bool, pub max_event_rate_hz: u32,
    pub name: MaxLen<&'i str, 16>,           // "IO4"
}
```

`TimedPinEvent` reuses `IoPinEvent`, so `Custom` events get timestamps too. `ww_gpio_hl` only needs its `Error`
matches extended; timed events and `read_inputs` / `write_outputs` are added to it in the same session.

## `ww_uart` 0.2

`ww_uart` 0.1 is a prototype used by `examples/uart`. 0.2 keeps the trait shape and the per-setting properties (D3),
and changes the two things that don't work: the TX sink and the RX item.

| | Item | Why |
|---|---|---|
| kept | `rx` stream at 0, `capabilities()`, properties `baud_rate`, `mode`, `stop_bits`, `parity`, `prevent_back_feed`, `reference_voltage`, `set_pin_level` | work |
| kept | `BaudRate`, `Mode` (incl. `Monitor2`, `Monitor4`, `HighZ`, `Test`), `Pin`, `StopBits`, `Parity`, `Capabilities` fields | work |
| kept | `tx_mon` stream, now defined: in `Monitor2` it carries the second line (rePort: CTS / RX2), in other modes an echo of what the device sent, if `capabilities` say so | covers the two-line monitor of §4.2 without a new stream; the host merges `rx` and `tx_mon` by `t` |
| changed | `sink!(tx: [u8])` → `#[credit] sink!(tx: MaxLen<RefVec<'i, u8>, 256>)` | a plain sink overruns the device's TX buffer on bulk writes (a file to the UART); API-28 credit sinks exist for exactly this. Wire break (resource kind changes) |
| changed | `RxChunk { flags: Option<RxFlags>, timestamp: Option<Second>, bytes }` → `RxChunk { seq, lost, t: Ticks, flags: RxFlags, bytes }` | the family stream head (D4-D6, D11); `Second` has no epoch, costs 3+ bytes and can't be correlated across streams. Wire break of the item |
| changed | `bytes: &'i [u8]` → `MaxLen<RefVec<'i, u8>, 512>` | bounded items (family rule); same bytes on the wire |
| added | `RxFlags`: `#[default] framing_error`, `overrun`, `break_detected` (appended after `parity_error`) | error flags of §4.2 |
| added | `property!(rw data_bits: DataBits, Error)`, `fn baud_rate_actual() -> u32`, `fn send_break(duration_us: u32)`, `fn tx_pending() -> u32` (appended) | §4.2; actual baud after the divider; break; TX drained |
| added | `Capabilities`: `#[default] data_bits`, `rx_buffer`, `tx_buffer` (appended) | |
| added | `Error`: `InUse`, `NoIoLevel`, `Refused`, `Timeout`, `#[future_version] Unknown(u32)` (appended) | common errors (D1, D8) |
| not taken | §4.2's `Config` struct and `configure()` | D3 |

```rust
#[ww_trait]
pub trait Uart {
    stream!(rx: RxChunk<'i>);                                  // 0  kept, item changed
    #[credit] sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>); // 1  credit sink now, UART_TX_MAX = 256
    stream!(tx_mon: RxChunk<'i>);                              // 2  kept, defined for Monitor2
    fn capabilities() -> Capabilities<'i>;                     // kept
    property!(rw baud_rate: BaudRate, Error);                  // kept ..
    property!(rw mode: Mode, Error);
    property!(rw stop_bits: StopBits, Error);
    property!(rw parity: Parity, Error);
    property!(rw prevent_back_feed: bool, Error);
    property!(rw reference_voltage: Volt, Error);
    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>; // .. kept
    property!(rw data_bits: DataBits, Error);                  // new: Seven, Eight (default), Nine, Five, Six
    fn baud_rate_actual() -> u32;                              // new
    fn send_break(duration_us: u32) -> Result<(), Error>;      // new
    fn tx_pending() -> u32;                                    // new: bytes not sent yet
}
pub struct RxChunk<'i> { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub flags: RxFlags,
                         pub bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX> }   // 512
```

Chunking (§4.2): a chunk ends after 1.5 characters of silence, at `UART_CHUNK_MAX` bytes or on an error flag; `t` is
the start bit of its first byte; `seq` counts bytes. `StopBits` (`u1`) and `Parity` (`u2`) are full or nearly so:
1.5 stop bits and mark / space parity would need a new enum, not planned.

## `ww_i2c` 0.2

`ww_i2c` 0.1 is a prototype; its method set (`i2c_configure`, `i2c_write`, `i2c_read` with `I2cReadKind`,
`i2c_scan`, `delay_after`) is the right one and stays, with names and order. Its commented `TODO`s already sketch
most of §4.3 (lines state, wiggle clock, cycle reads); 0.2 fills them in.

| | Item | Why |
|---|---|---|
| kept | `i2c_capabilities`, `i2c_configure(mode, io_level_mv) -> u32`, `i2c_write(addr, data, delay_after)`, `i2c_read(addr, kind, len, delay_after)`, `i2c_scan`, order and names | work; `delay_after` is the start of IF-6 polling |
| kept | `u8` 7-bit addresses (D12), `I2cReadKind`, `I2cMode` (`self_describing`, all four modes), `I2cCapabilities` fields, all `I2cError` variants | work |
| changed | `Vec<u8>` → `MaxLen<RefVec<'i, u8>, I2C_MAX>` (512) in `i2c_write` and `i2c_scan` (128) | a `no_alloc` device server can't take a `Vec`; bounded sizes (family rule). Adding a bound is a break |
| changed | `RawTimestamp { ticks: u32 }` → `ww_timebase::Ticks` in `I2cTransactionTimeRange` | one timebase; `u32` wraps after 71 minutes at 1 MHz |
| changed | the plain structs (`I2cTransactionTimeRange`, `I2cReadEnvelope`, `I2cCapabilities`, `I2cLinesState`) get `#[derive_shrink_wrap]`, `I2cReadEnvelope` its missing `<'i>`, `I2cCapabilities::driver_name` a `MaxLen<&'i str, 32>` | they cross the wire, so they must be proper types; strings bounded |
| changed | `I2cError` loses `self_describing` (becomes `Unsized`) | SW-16 `Unknown(u32)` and future variants with data (`Nack { at }`) |
| added | `i2c_transaction(addr, ops)` (appended) | §4.3: repeated starts, embedded-hal `transaction` |
| added | `i2c_lines_state() -> I2cLinesState`, `i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError>` (appended, the names of the 0.1 `TODO`s) | §4.3 `lines` / `recover` |
| added | `stream!(i2c_monitor: MonitorRecord)` (appended) | MON-1: `MonitorOnly` and `Master { monitor: true }` already exist as modes, the stream was missing |
| added | `property!(rw i2c_pullups: bool, I2cError)` (appended), `I2cLinesState`: `#[default] scl_mv`, `sda_mv: Option<u16>` | pull-ups; bus voltages when measurable |
| added | `I2cError`: `InUse`, `NoIoLevel`, `Refused`, `AddrNack`, `#[future_version] Unknown(u32)` (appended) | common errors (D1, D8); `Nack` stays for data NACKs |
| later | cycle reads (`i2c_cycle_read` and friends stay commented) | IF-6 i2c-polling, additive |

```rust
#[ww_trait]
pub trait I2c {
    fn i2c_capabilities() -> I2cCapabilities<'i>;                                                  // kept
    fn i2c_configure(mode: I2cMode, io_level_mv: u16) -> Result<u32, I2cError>;                    // kept
    fn i2c_write(addr: u8, data: MaxLen<RefVec<'i, u8>, I2C_MAX>, delay_after: u32)
        -> Result<I2cTransactionTimeRange, I2cError>;                                              // Vec -> MaxLen
    fn i2c_read(addr: u8, kind: I2cReadKind<'i>, len: u16, delay_after: u32)
        -> Result<I2cReadEnvelope<'i>, I2cError>;                                                  // kept
    fn i2c_scan() -> Result<MaxLen<RefVec<'i, u8>, 128>, I2cError>;                                // Vec -> MaxLen
    /// Several operations to one address with repeated starts, one stop at the end
    fn i2c_transaction(addr: u8, ops: MaxLen<RefVec<'i, I2cOp<'i>>, 16>)
        -> Result<I2cTransactionResult<'i>, I2cError>;                                             // new
    fn i2c_lines_state() -> I2cLinesState;                                                         // new
    /// Up to `count` clocks on SCL, then a stop: frees a slave stuck mid-byte
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError>;                            // new
    stream!(i2c_monitor: MonitorRecord<'i>);                                                       // new
    property!(rw i2c_pullups: bool, I2cError);                                                     // new
}
pub enum I2cOp<'i> { Write(MaxLen<RefVec<'i, u8>, I2C_MAX>), Read(u16) }
pub struct I2cTransactionResult<'i> { pub time: I2cTransactionTimeRange,
                                      pub reads: MaxLen<RefVec<'i, MaxLen<RefVec<'i, u8>, I2C_MAX>>, 16> }
pub struct MonitorRecord<'i> { pub seq: u32, pub lost: UNib32, pub t: Ticks, pub t_end: Ticks, pub addr: u8,
                               pub read: bool, pub kind: RecordKind, pub nack_at: Option<u16>,
                               pub bytes: MaxLen<RefVec<'i, u8>, I2C_MAX> }
pub enum RecordKind { Complete, RepeatedStart, BusError }   // unib32, grows
```

One monitor record per transaction keeps the stream compact (about 10 bytes plus the data); `seq` counts records.

## `ww_spi` 0.2

`ww_spi` 0.1 is an empty trait, so there is nothing to keep; designed from §4.4 with the family conventions.

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
    property!(rw reference_voltage: Volt, Error);         // as ww_uart (D2)
}
pub struct SpiConfig { pub role: Role, pub freq_hz: u32, pub mode: SpiMode, pub bit_order: BitOrder,
                       pub word_bits: u8, pub cs: CsMode }
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

- `ww_gpio` 0.1 → 0.2: wire-compatible for everything 0.1 sends; only the appended `Error` variants are new to 0.1
  readers, and exhaustive Rust matches on `Error` need the new arms. `ww_gpio_hl` and the gpio examples barely change.
- `ww_uart` 0.1 → 0.2: **wire break** of `tx` (credit sink) and of the `RxChunk` item; Rust users of the properties
  are unaffected. Affected: `examples/uart*`.
- `ww_i2c` 0.1 → 0.2: **wire break** of `i2c_write` / `i2c_scan` (bounds), the timestamps and `I2cError`. No user in
  this repo.
- `ww_spi` 0.1 → 0.2: the empty trait gets its content.
- Outside this repo: `usb_io_fw` moves to the family in its 0.4.0 (FW-1); `examples_mcu/{mcu_qemu, nucleo_h743zi2,
  usb_stm32h725ig}` use `ww_gpio` and follow. No deployed device besides rePort rev A boards in-house uses them.
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

Risks the skeleton checks first: `RefVec` of `MaxLen<RefVec<..>>` in method
returns through the codegen (no test uses them yet); `#[credit] sink!` of a stdlib trait used by `ww_impl!`; appending
`#[future_version] Unknown(u32)` to the 0.1 error enums once SW-16 is there.

## Ideas not asked for

- **Loopback mocks as a reusable simulator** (`ww_io_sim`, std): the test servers of step 2, wired TX↔RX,
  IO0↔IO4, an I2C EEPROM at 0x50, a 1-wire DS18B20; WW-4 runs crates.io embedded-hal drivers against it and the
  rePort sim (HOST-2) reuses it.
- **`ww io` CLI subcommands** (TOOL-8 and friends): `ww gpio`, `ww uart term`, `ww i2c scan`, `ww adc read` on any
  device that has the trait, found by global trait path: `DynClient::traits_only(..)` and
  `DynClient::global_attachment(ww_x::TRAIT_FULL_GID, "Trait")` (API-20 / CLT-10, WW-2, branch
  `s/14oct2026-ww-cross-version`). The device resolves the first instance outside arrays, so each IO trait sits at
  most once per device tree and never inside a trait array (rePort's tree already does that).
- **sigrok export** of monitor streams on the host (`seq` / `t` / `lost` map to it directly).
