# ww_spi

Part of the [IO trait family](io_family.md). `ww_spi` is a global WireWeaver trait for talking to the SPI devices
on a bus remotely: full-duplex transfers, transactions with CS held across several operations, plain writes, and
watching a bus without taking part. It is `no_std` by default and depends only on `wire_weaver` and `ww_si` (for
the `Hertz`, `Second` and `Volt` quantity types).

Add it to a device API crate with:

```toml
[dependencies]
ww_spi = "0.1"
```

## Traits

| Trait        | What it is for                                                        | Who implements it         |
|--------------|------------------------------------------------------------------------|----------------------------|
| `Spi`        | Configure the bus, full-duplex transfers, transactions with CS held, writes. | Every SPI master     |
| `SpiMonitor` | Every frame (one CS assertion) on the bus as a stream.                | Sniffers, debug adapters  |

Monitoring is its own trait because it is its own function: a sniffer may have no master at all. A device picks
the traits it implements:

```rust,ignore
#[ww_api_root]
trait MyAdapter {
    ww_impl!(spi: ww_spi::Spi);
    ww_impl!(spi_monitor: ww_spi::SpiMonitor);
}
```

### `Spi`

```rust
#[ww_trait]
pub trait Spi {
    fn transfer(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError>;

    fn transaction(ops: MaxLen<RefVec<'i, SpiOp<'i>>, MAX_OPS>) -> Result<MaxLen<RefVec<'i, u8>, SPI_MAX>, SpiError>;

    fn write(data: MaxLen<RefVec<'i, u8>, SPI_MAX>) -> Result<(), SpiError>;

    fn capabilities() -> SpiCapabilities<'i>;

    fn configure(config: SpiConfig) -> Result<SpiActual, SpiError>;

    fn set_cs(active: bool) -> Result<(), SpiError>;

    property!(rw reference_voltage: Volt, SpiError);
}
```

* `transfer` - full duplex: writes `data`, returns as many bytes as were read. CS is asserted for the call and
  released after (`CsMode::Auto`); with `CsMode::Manual` the bus must already be selected through `set_cs`.
* `transaction` - CS asserted once for every op of `ops`, released at the end; returns the bytes of every
  `SpiOp::Read` and `SpiOp::Transfer`, concatenated in order. This is how a command-then-response exchange (write a
  register address, then read it) stays under one CS assertion instead of two separate `transfer` calls.
* `write` - like `transfer`, ignoring what comes back.
* `capabilities` - frequency range, word sizes, master/slave support, IO levels; see [Types](#types) below.
* `configure` - sets the bus (role, frequency, mode 0-3, bit order, word size, CS mode) and returns the frequency
  actually set, the nearest the hardware can do without going above what was asked. `SpiRole::Disabled` releases
  the pins.
* `set_cs` - asserts or releases CS, only meaningful with `CsMode::Manual`.
* `reference_voltage` - IO level, on adapters with level translators.

### `SpiMonitor`

```rust
#[ww_trait]
pub trait SpiMonitor {
    #[stream(SpiFrame<'i>, counted)]
    fn frames(config: SpiMonitorConfig) -> Result<(), SpiError>;
}
```

* `frames` - a counted [stream function](../api/streams.md#opening-a-stream): the host opens it with the bus
  settings needed to decode frames (mode, bit order, word size) and receives one `SpiFrame` per CS assertion seen on
  the bus, in order, until it closes the stream. The monitor doesn't drive CS or the clock itself, it only listens.
  Its unit is a frame: the event head carries when CS was asserted and how many frames the device dropped
  ([stream delivery](../api/stream_reliability.md)).

## Types

* `SpiConfig` - `role: SpiRole`, `freq: Hertz`, `mode: SpiMode`, `bit_order: BitOrder`, `word_bits: u8`,
  `cs: CsMode`; given to `Spi::configure`.
* `SpiActual` - `freq: Hertz`, the frequency `Spi::configure` actually set.
* `SpiMonitorConfig` - `mode`, `bit_order`, `word_bits`: the subset of `SpiConfig` a monitor needs to decode frames,
  the argument of `SpiMonitor::frames`.
* `SpiRole` - `Disabled`, `Master`, `Slave`. Data paths for slave mode are not defined yet.
* `SpiMode` - `Mode0`..`Mode3`, the usual CPOL/CPHA numbering; `sized`, 2 bits.
* `BitOrder` - `MsbFirst`, `LsbFirst`; `sized`, 1 bit.
* `CsMode` - `Auto` (asserted per transfer/transaction, released after), `Manual` (only `Spi::set_cs`), `None` (not
  driven by this device).
* `SpiOp<'i>` - one operation of a `Spi::transaction`: `Write`, `Read(u16)`, `Transfer`, `Delay(Second)`.
* `SpiFrame<'i>` - one frame seen by `SpiMonitor`: `end: Second` (CS released; asserted is the head's time), `truncated`,
  `mosi` / `miso: MaxLen<RefVec<'i, u8>, SPI_MAX>`.
* `SpiCapabilities<'i>` - `min_freq` / `max_freq: Hertz`, `min_word_bits` / `max_word_bits: u8`, `master` /
  `slave` / `monitor: bool`, `io_levels: MaxLen<RefVec<'i, Volt>, 8>`, `max_len: u32` (largest transfer this
  device does, at most `SPI_MAX`).
* `SpiError` - `Unsupported`, `InUse`, `NoIoLevel`, `Refused`, `InvalidArgument`, `WrongMode`, `BufferTooBig`,
  `Timeout`; unsized (`unib32` discriminant), evolvable. Each method's rustdoc lists the variants it returns.

`SPI_MAX` (512 bytes) bounds every transfer, write and the reads of one transaction concatenated; `MAX_OPS` (16)
bounds the operations of one `Spi::transaction`. Both are compatible to raise later, not to lower.
