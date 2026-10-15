# ww_uart

Part of the [IO trait family](io_family.md). `ww_uart` is a WireWeaver trait for bridging a UART (serial port) to
the host, and for watching both lines of a serial link without driving them. It is `no_std` by default and depends
on `wire_weaver` and `ww_si` (for `Baud` rates, the `Volt` reference-voltage type and `Second` timestamps).

Add it to a device API crate with:

```toml
[dependencies]
ww_uart = "0.1"
```

## Traits

`Uart` is one port: configure it, receive on `rx`, send on the `tx` credit sink, and, on ports with the hardware for
it, watch both lines of a link between two other devices on the [optional](../api/optional_items.md) `monitor`
stream. It composes with the rest of the API using [`ww_impl!`](../api/traits.md) and is nested as an
[array](../api/arrays.md) when a device has more than one UART (`ww_impl!(uart[]: ww_uart::Uart)`, as
`examples/uart_api` does); monitoring comes with each element, no second array or trait to keep in step.

### `Uart`

```rust
#[ww_trait]
pub trait Uart {
    #[counted]
    stream!(rx: RxChunk<'i>);

    #[credit]
    sink!(tx: MaxLen<RefVec<'i, u8>, UART_TX_MAX>);

    fn capabilities() -> Capabilities<'i>;

    fn configure(config: UartConfig) -> Result<UartConfig, Error>;

    fn set_pin_level(pin: Pin, is_high: bool) -> Result<(), Error>;
    fn send_break(duration: Second) -> Result<(), Error>;
    fn tx_pending() -> u32;

    #[optional]
    #[stream(MonitorChunk<'i>, counted)]
    fn monitor(config: MonitorConfig) -> Result<MonitorConfig, Error>;
}
```

* `rx` - received bytes, chunked as they arrive (see [`RxChunk`](#types) below). A chunk ends after a short
  silence on the line, when it is full, or on a line error. A counted stream, opened by calling it: its unit is a
  byte, so the event head carries each chunk's byte position, the device time of its first byte, and exactly how
  many bytes the device dropped before it ([stream delivery](../api/stream_reliability.md)); the chunk itself says
  when its last byte arrived ([Time](#time) below).
* `tx` - bytes to send, a [credit sink](../api/streams.md) (API-28): the host tracks the device's TX buffer
  (`Capabilities::tx_buffer`) and never sends more than it holds, so a large write (e.g. a file piped to the UART)
  can't overrun it.
* `capabilities()` - what this device supports: baud rate range, flow control modes, IO levels, buffer sizes.
* `configure` - the whole line configuration in one [`UartConfig`](#types) (baud rate, data bits, parity, stop bits,
  mode, back-feed detector, IO level), answered with what was actually set: the hardware rounds the baud rate to
  what its divider can do, and `io_level: None` keeps the level and is answered with the level in effect. One call
  instead of a property per setting, so a port never runs with half a configuration.
* `set_pin_level` - manual control of a pin, only in [`Mode::Test`](#types); every other mode answers
  `Common(WrongState)`.
* `send_break` - holds TX low for `duration` (a break condition), after the bytes already queued.
* `tx_pending()` - bytes queued for sending and not sent yet.
* `monitor` - `#[optional]`, a counted [stream function](../api/streams.md#opening-a-stream): the host opens it with the line
  settings of the watched link (baud rate, data bits, parity, stop bits, IO level) and gets back what was actually
  set (as `Uart::configure` does), then both directions of the link (rePort: RX and CTS/RX2) in one stream, in
  the order they arrived: each `MonitorChunk` says which `MonitorLine` it came from, so a request and its answer
  stay in order. Closing the stream stops receiving. The unit is a byte of either line. Most bridges can't sniff a
  link they aren't part of: such a device leaves it out with `omit = ["uart.monitor"]` in its `ww_codegen!`, and a
  host sees that in the device's introspection.

## Time

Every received chunk, on `rx` and on `monitor`, says when it arrived, on the device's one clock
([`ww_timebase::Clock`](ww_timebase.md)) that also timestamps everything else the device captures:

| What | Where | |
|---|---|---|
| first byte arrived (end of its stop bit) | the event head's `t` | always set |
| last byte arrived | `RxChunk::end: Second` | in the item |

Both are `ww_si::Second`s since the device's clock started, in the prefix and number type the device picks (usually
a `u32` of microseconds, which wraps every 71.6 minutes; the host unwraps it around `Clock::now()`). With them a host:

- lines up serial traffic with the device's other data: a GPIO edge, an ADC sample, an I2C transaction;
- tells a request and its answer apart in time on a `monitor` link, and measures the turnaround between them;
- sees the gaps on the line: a chunk ends on a silence, so the next chunk's first byte comes at least that silence
  after the previous chunk's `end`.

On the client the head's time is the `t` of `StreamEvent::Item` (`rx.recv_event()`), next to the chunk:

```rust
let mut rx = client.uart().rx().call().await?;
loop {
    match rx.recv_event().await? {
        StreamEvent::Item { t: Some(first), value, .. } => {
            println!("{} bytes from {:.6} s to {:.6} s", value.bytes.len(), first.to_f64(), value.end.to_f64());
        }
        StreamEvent::Gap { units, .. } => println!("{units} bytes lost"),
        _ => {}
    }
}
```

A device that can't time each byte (a DMA that hands over a block when the line goes idle) times the block's end
and takes the first byte a character time per byte earlier at the configured baud rate, as
`examples_mcu/usb_stm32h725ig` does.

## Types

* `ww_si::Baud` - a baud rate (`Baud::baud(115_200u32)`), in the number type and prefix the caller picks, like every
  other `ww_si` quantity.
* `Mode` - `Asynchronous` (no flow control), `HardwareFlowControl` (RTS/CTS), `SoftwareFlowControl` (XON/XOFF),
  `HighZ` (all pins High-Z), `Test` (manual pin control through `set_pin_level`).
* `Pin` - `Tx`, `Rx`, `Rts`, `Cts`, used by `set_pin_level`.
* `StopBits` - `Stop1`, `Stop2`.
* `Parity` - `None`, `Odd`, `Even`.
* `DataBits` - `Eight`, `Seven`, `Nine`, `Six`, `Five`.
* `UartError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)); unsized, evolvable. Each
  method lists the variants it returns (`#[errors]`); line errors of received data are `RxFlags` of the chunk, not
  errors.
* `RxChunk<'i>` - `end: Second` (when the last byte arrived), `flags: Option<RxFlags>`, `bytes: MaxLen<RefVec<'i,
  u8>, UART_CHUNK_MAX>`; when the first byte arrived is the time in the event head, see [Time](#time).
* `MonitorChunk<'i>` - `line: MonitorLine` (`A` or `B`, 1 bit), `chunk: RxChunk<'i>`.
* `RxFlags` - `parity_error`, `framing_error`, `overrun`, `break_detected`, seen while receiving a chunk.
* `UartConfig` - `baud_rate: Baud`, `data_bits`, `parity`, `stop_bits`, `mode`, `prevent_back_feed: bool`,
  `io_level: Option<Volt>`; the argument and the answer of `configure`.
* `MonitorConfig` - `baud_rate`, `data_bits`, `parity`, `stop_bits`, `io_level` of the link `Uart::monitor`
  watches; the argument and the answer of `monitor`.
* `Capabilities<'i>` - `min_baud_rate` / `max_baud_rate: Baud`, `io_levels: MaxLen<RefVec<'i, Volt>,
  MAX_IO_LEVELS>`, `io_level_min` / `io_level_max: Option<Volt>` (an adjustable supply),
  `hw_flow_control` / `sw_flow_control` / `high_z_mode` / `test_mode` /
  `back_feed_detector: bool`, `rx_buffer` / `tx_buffer: u32` (device receive and transmit buffer sizes, in bytes).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")`, so owned (`*Owned`)
variants are available by enabling the crate's `std` feature; the plain configuration enums (`Mode`,
`Pin`, `StopBits`, `Parity`, `DataBits`) also derive `PartialEq`/`Eq`.

## Composing into a device API

A device exposes a UART by implementing `Uart` on its API root:

```rust
#[ww_api_root]
pub trait MyDeviceApi {
    ww_impl!(uart: ww_uart::Uart);
}

// a port without monitoring hardware
ww_codegen!(my_device_api::MyDeviceApi for Server, server = true, omit = ["uart.monitor"]);
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

Receiving chunks from `rx` and configuring the port look the same as any other nested trait, method and
stream (see [streams](../api/streams.md)); for a non-array `uart` resource (as in `tests/io_uart_api`) the same
calls drop the index: `client.uart().rx()`, `client.uart().configure(config)`.
