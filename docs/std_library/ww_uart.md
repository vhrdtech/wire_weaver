# ww_uart

Part of the [IO trait family](io_family.md). `ww_uart` is a WireWeaver trait for bridging a UART (serial port) to
the host, and for watching both lines of a serial link without driving them. It is `no_std` by default and depends
on `wire_weaver` and `ww_si` (for the `Volt` reference-voltage type and `Second` timestamps).

Add it to a device API crate with:

```toml
[dependencies]
ww_uart = "0.1"
```

## Traits

| Trait | What it is for | Who implements it |
|---|---|---|
| `Uart` | Configure, receive on `rx`, send on the `tx` credit sink. | Every UART bridge |
| `UartMonitor` | Both lines of a serial link as two streams, without driving them. | Sniffers, debug adapters |

Both compose with the rest of the API using [`ww_impl!`](../api/traits.md) and can be nested as
[arrays](../api/arrays.md) when a device has more than one UART (`ww_impl!(uart[]: ww_uart::Uart)`, as
`examples/uart_api` does).

### `Uart`

```rust
#[ww_trait]
pub trait Uart {
    #[counted]
    stream!(rx: RxChunk<'i>);

    #[credit]
    sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);

    fn capabilities() -> Capabilities<'i>;

    property!(rw baud_rate: BaudRate, Error);
    property!(rw mode: Mode, Error);
    property!(rw stop_bits: StopBits, Error);
    property!(rw parity: Parity, Error);
    property!(rw data_bits: DataBits, Error);
    property!(rw prevent_back_feed: bool, Error);
    property!(rw reference_voltage: Volt, Error);

    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>;
    fn baud_rate_actual() -> u32;
    fn send_break(duration: Second) -> Result<(), Error>;
    fn tx_pending() -> u32;
}
```

* `rx` - received bytes, chunked as they arrive (see [`RxChunk`](#types) below). A chunk ends after a short
  silence on the line, when it is full, or on a line error. A counted stream, opened by calling it: its unit is a
  byte, so the event head carries each chunk's byte position, the device time of its first byte, and exactly how
  many bytes the device dropped before it ([stream delivery](../api/stream_reliability.md)).
* `tx` - bytes to send, a [credit sink](../api/streams.md) (API-28): the host tracks the device's TX buffer
  (`Capabilities::tx_buffer`) and never sends more than it holds, so a large write (e.g. a file piped to the UART)
  can't overrun it.
* `capabilities()` - what this device supports: baud rate range, flow control modes, reference voltages, buffer
  sizes.
* `baud_rate` / `mode` / `stop_bits` / `parity` / `data_bits` / `prevent_back_feed` / `reference_voltage` - line
  configuration; the hardware rounds the requested baud rate, so `baud_rate_actual()` says what it actually set.
* `set_pin_level` - manual control of a pin, only in [`Mode::Test`](#types); every other mode answers
  [`Error::WrongMode`](#types).
* `send_break` - holds TX low for `duration` (a break condition), after the bytes already queued.
* `tx_pending()` - bytes queued for sending and not sent yet.

### `UartMonitor`

```rust
#[ww_trait]
pub trait UartMonitor {
    #[stream(MonitorChunk<'i>, counted)]
    fn lines(config: MonitorConfig) -> Result<u32, Error>;
}
```

* `lines` - a counted [stream function](../api/streams.md#opening-a-stream): the host opens it with the line
  settings of the watched link (baud rate, data bits, parity, stop bits) and gets back the baud rate actually set
  (same as `Uart::baud_rate_actual`), then both directions of the link (rePort: RX and CTS/RX2) in one stream, in
  the order they arrived: each `MonitorChunk` says which `MonitorLine` it came from, so a request and its answer
  stay in order. Closing the stream stops receiving. The unit is a byte of either line.

Monitoring is its own trait because it is its own function: a sniffer may have no UART master at all, and most
bridges can't sniff a link they aren't part of.

## Types

* `BaudRate` - common rates (`Baud110` .. `Baud921600`) encode in a nibble; anything else is
  `BaudOther(u32)` in baud.
* `Mode` - `Asynchronous` (no flow control), `HardwareFlowControl` (RTS/CTS), `SoftwareFlowControl` (XON/XOFF),
  `HighZ` (all pins High-Z), `Test` (manual pin control through `set_pin_level`).
* `Pin` - `Tx`, `Rx`, `Rts`, `Cts`, used by `set_pin_level`.
* `StopBits` - `Stop1`, `Stop2`.
* `Parity` - `None`, `Odd`, `Even`.
* `DataBits` - `Eight`, `Seven`, `Nine`, `Six`, `Five`.
* `Error` - `Unsupported`, `UnsupportedMode`, `UnsupportedReferenceVoltage`, `WrongMode`, `CustomU8(u8)`,
  `CustomU32(u32)`, `InUse`, `NoIoLevel`, `Refused`, `Timeout`; each method's rustdoc lists the variants it returns.
* `RxChunk<'i>` - `flags: Option<RxFlags>`, `bytes: MaxLen<RefVec<'i, u8>, UART_CHUNK_MAX>`; when it was received
  is the time in the event head.
* `MonitorChunk<'i>` - `line: MonitorLine` (`A` or `B`, 1 bit), `chunk: RxChunk<'i>`.
* `RxFlags` - `parity_error`, `framing_error`, `overrun`, `break_detected`, seen while receiving a chunk.
* `MonitorConfig` - `baud_rate`, `data_bits`, `parity`, `stop_bits` of the link `UartMonitor::lines` watches.
* `Capabilities<'i>` - `min_baud_rate` / `max_baud_rate: u32`, `voltages: MaxLen<RefVec<'i, Volt>, 8>`,
  `rx_timestamps` (`rx` events carry their time) / `hw_flow_control` / `sw_flow_control` / `high_z_mode` / `test_mode` /
  `back_feed_detector: bool`, `rx_buffer` / `tx_buffer: u32` (device receive and transmit buffer sizes, in bytes).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")`, so owned (`*Owned`)
variants are available by enabling the crate's `std` feature; the plain configuration enums (`BaudRate`, `Mode`,
`Pin`, `StopBits`, `Parity`, `DataBits`) also derive `PartialEq`/`Eq`.

## Composing into a device API

A device exposes a UART by implementing `Uart` (and, for a sniffer, `UartMonitor`) on its API root:

```rust
#[ww_api_root]
pub trait MyDeviceApi {
    ww_impl!(uart: ww_uart::Uart);
    ww_impl!(uart_monitor: ww_uart::UartMonitor);
}
```

`tests/io_uart_api` is exactly this shape (used by `tests/io_uart`'s mock server and in-process tests); a device
with more than one UART nests it as an array instead (`ww_impl!(uart[]: ww_uart::Uart)`, see `examples/uart_api`).

## Client usage (`examples/uart`)

The generated client wraps `wire_weaver_client::Commander`; see `examples/uart/src/lib.rs` for the small
`UartBridge` wrapper type (connecting over USB with a fixed VID/PID). Sending bytes through the `tx` credit sink
(`examples/uart/examples/simple.rs`):

```rust
let device = UartBridge::new().connect().await?;
let mut tx = device.uart(0).tx().await?;
tx.send_bytes(b"abc000def").await?;
```

Receiving chunks from `rx` and reading/writing properties looks the same as any other nested trait and credit
sink/stream (see [properties](../api/properties.md) and [streams](../api/streams.md)); for a non-array `uart`
resource (as in `tests/io_uart_api`) the same calls drop the index: `client.uart().rx()`,
`client.uart().write_baud_rate(BaudRate::Baud115200)`.
