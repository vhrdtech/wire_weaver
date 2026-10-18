---
level: basic
---

# Streams

A stream carries a sequence of values in one direction. Naming is from the device's point of view: a **stream** goes
out of the device to the host (`stream!`), a **sink** goes into the device from the host (`sink!`).

```rust
#[ww_trait]
trait MyDevice {
    stream!(button: ButtonEvent);
    stream!(uart_rx: [u8]);
    sink!(uart_tx: [u8]);
    sink!(leds: Vec<LedState>);
}
```

Any type the SerDes system supports works as a stream item. Items keep their boundaries: each value sent arrives as
one value, in order, even when several are packed into one packet on the way. Sending `[1, 2, 3]`, `[4]`, `[5, 6]`
on `uart_rx` gives the same three slices on the other end. A byte item (`[u8]`, `Vec<u8>`, or
[`MaxLen<RefVec<'i, u8>, N>`](../types.md#bounded-lists-and-strings-maxlent-n) to bound it) is sent as is, without a
length prefix.

Streams and sinks come in kinds that differ in what they promise about delivery:

| Kind | Declared as | What is guaranteed |
|---|---|---|
| plain stream | `stream!(name: T)` | nothing: items go out once, a lost one is gone unnoticed; the latest value wins |
| counted stream | `#[counted] stream!(..)`, `#[stream(T, counted)] fn ..` | every item has a position: the host learns exactly how much was lost, where and why |
| reliable stream | `#[reliable] stream!(..)`, `#[stream(T, reliable)] fn ..` | counted, acknowledged and resent; kept in a store on the device while the host is slow or gone |
| plain sink | `sink!(name: T)` | nothing: writes are not acknowledged, the device keeps up or drops |
| credit sink | `#[credit] sink!(name: T)` | flow control: the host never sends more than the device has room for, lost writes are resent |

The kind is part of the API and visible in introspection, so it is chosen in the API crate. This page shows the
common case, plain streams and sinks, and what every kind shares. The others have pages of their own:
[Counted and reliable streams](counted_streams.md) (what they guarantee: [Stream delivery](stream_reliability.md))
and [Credit sinks and sideband](credit_sinks.md).

## Stream functions

A stream is opened by calling it, like a method: the host sends its arguments, the device decides whether to start,
and answers with a result. A stream that takes arguments or returns a result is declared as a **stream function**, a
trait `fn` with a `#[stream(ItemType)]` or `#[stream(ItemType, counted | reliable)]` attribute:

```rust
#[ww_trait]
trait Adc {
    /// Sample sets at `config.rate`; unit: sample set
    #[errors(BadRate, Common(InUse))]
    #[stream(SampleBlock<'i>, reliable)]
    fn samples(config: StreamConfig) -> Result<StreamActual, AdcError<'i>>;

    /// Bytes as they arrive on the line; unit: byte
    #[counted]
    stream!(rx: [u8]);
}
```

`stream!(name: T)` is the short form of a stream function without arguments and result, `#[counted]` /
`#[reliable]` above it choose its kind. The return type is either nothing or a `Result<A, E>`: `Ok` opens the stream
and gives the host `A` (e.g. the rate the device actually picked), `Err` answers with the error and leaves the stream
as it was. Calling an open stream again opens it again, a new session, e.g. with another configuration.

`#[errors(..)]` above a stream function lists the variants of `E` opening it returns, as on methods (see
[Errors](errors.md#which-errors-a-method-returns)); a `stream!` has no result, so no list. `#[optional]` /
`#[optional(group)]` lets a device leave a stream or sink out (see [Optional items](optional_items.md)). Counted
streams document their **unit** (byte, sample set, transaction), the doc comments above do so with `unit: ..`.

## Device side

`ww api scaffold <api_crate>` prints every handler and state field a server needs, with the right signatures for
the `ww_codegen!` options (see [`ww api`](../cli_reference.md#ww-api)). For a stream named `samples` they are:

| Handler | Plain | Counted | Reliable | Called |
|---|---|---|---|---|
| `fn samples(&mut self, cx, args..) -> RpcResult<R>` | ✓ | ✓ | ✓ | the host opens the stream |
| `fn close_samples(&mut self, cx)` | ✓ | ✓ | ✓ | the host closes it |
| `fn sideband_samples(&mut self, cx, sideband) -> Option<StreamSideband>` | ✓ | | | other sideband from the host, see [Sideband](credit_sinks.md#sideband) |
| `fn stream_samples(&mut self) -> &mut Counted` / `&mut Reliable<S>` | | ✓ | ✓ | whenever generated code needs the stream's state |
| `fn new_stream_session(&mut self) -> u32` (one per server) | | ✓ | ✓ | a host opens a counted or reliable stream |

All handlers get `cx: &mut Context<'_, impl EventOut, Medium>` (`impl BlockingEventOut` for `use_async = false`);
streams in [arrays](#arrays-of-streams) get their index chain after it. The open handler answers right away:
`Ready(..)` with the result, `Unimplemented` refuses it; a deferred answer is not supported for an open. How
counted and reliable streams keep their state and send is in
[Counted and reliable streams](counted_streams.md#counted-streams).

### Plain streams

The generated `stream_data_ser()` has, for every plain stream, `<name>_send(&value, out)` (async),
`<name>_send_blocking(&value, out)`, and `<name>(&value, scratch)` returning the serialized event. `out` is the `cx` of
a handler, or `server.sink()` of the `ww_device` server anywhere in the event loop (in the snippets, `server` is the
`ww_device` server and `state` the struct the API is implemented on):

```rust
impl ServerState {
    async fn button(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.button_open = true;
        Ready(())
    }

    async fn close_button(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        self.button_open = false;
    }
}

// from a handler, goes out before its reply
server_impl::stream_data_ser().button_send(&event, cx).await?;
// from the event loop
if state.button_open {
    server_impl::stream_data_ser().uart_rx_send(&chunk, &mut server.sink()).await?;
}
```

Nothing stops the device from sending to a plain stream no host opened: the host drops events no handle waits for.
Keeping track of whether a host listens is up to the open and close handlers.

## Host side

Generated clients have one function per stream. A stream function prepares the open; `.call().await` (or
`.blocking_call()`) sends it and returns the stream handle, together with the function's result when it has one:

```rust
// stream!(button: ButtonEvent): a plain stream, Stream<ButtonEvent>
let mut button = device.button().call().await?;
// a counted stream, PositionedStream<TailBytesOwned>
let mut rx = device.rx().call().await?;
// a fallible stream function: Result<(StreamActual, PositionedStream<..>), AdcError> inside the call's Result
let (actual, mut samples) = device.samples(config).call().await??;
```

Before `.call()`, the prepared open takes `.with_timeout(..)`, `.queue_bytes(..)` (how much the host queues before it
drops, plain and counted streams), `.window(..)` and `.ack_mode(..)` (reliable streams). Only what the device sends
after the open reaches the handle; events before it are not buffered.

### Plain streams: `Stream<T>`

- `recv()` returns the next value, `recv_blocking()` and `try_recv()` (`None` when nothing is there yet) the same
  without async. They skip `Connected` and fail on a sideband event or a disconnect.
- `recv_any()` (`recv_any_blocking()`) returns every event: `TypedStreamEvent::Data(value)`, `Sideband(..)`,
  `Connected`, `Disconnected`, `Dropped { events, bytes }`.
- A byte stream (`Stream<TailBytesOwned>`) has `recv_all_bytes()`, collecting everything until the device sends
  `Close`, with `_blocking` and `_timeout` variants.
- `close()` closes the stream on the device. Dropping the handle doesn't.

The host queues at most `ClientConfig::stream_queue_bytes` (4 MiB by default) of a stream's items when user code
doesn't read; what comes beyond is dropped, counted in `dropped()` and reported once as `Dropped`.

Counted and reliable streams return a `PositionedStream<T>` instead, with positions, gaps and resuming, see
[Counted and reliable streams](counted_streams.md#host-side-positionedstreamt).

## Sinks

A sink is written by the host, one value per write:

```rust
// host
let mut leds = device.leds().await?;            // Sink<Vec<LedState>>, `leds_blocking()` without async
leds.send(vec![LedState::On, LedState::Off]).await?;
let mut tx = device.uart_tx().await?;           // Sink<TailBytesOwned>
tx.send_bytes(b"hello").await?;
```

`send()` / `send_blocking()` take a borrowed value, `send_owned()` an owned type (`FooOwned`), `send_bytes()` the
bytes of a byte sink; `try_send()` (`try_send_owned()`, `try_send_bytes()`) returns `Ok(false)` instead of waiting,
for GUIs and promise-style code.

On the device, each write calls `write_<name>(cx, value)`; a byte sink gets the bytes as `&TailBytes<'_>`, a byte sink
with a `MaxLen` bound refuses a longer write before the handler sees it:

```rust
fn write_uart_tx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, data: &TailBytes<'_>) {
    if self.tx_ring.push_slice(data.0).is_err() {
        self.tx_dropped += 1; // the device can't push back on a plain sink
    }
}
```

A plain sink's writes are sent without a request id: the device sends nothing back, and the host doesn't wait. A
plain sink needs no opening, `send()` works right away; `open()` and `close()` send the `Open` and `Close` sideband,
which reach the device's `sideband_<name>` handler, for devices that want to know.

## Flow control

What keeps a fast side from overrunning a slow one depends on the kind:

- **Plain streams**: nothing. The device sends what it has, the host queues up to `stream_queue_bytes` per stream and
  drops beyond it.
- **Counted streams**: nothing slows the device down either, but every loss, on the device, on the link or in the
  host's queue, is reported with its size and place.
- **Reliable streams**: the host grants the device a window of bytes in flight and acknowledges what it received; a
  host that doesn't read stops granting, and the device keeps the data in its store (see
  [Acknowledgements and resends](stream_protocol.md#acknowledgements-and-resends)).
- **Plain sinks**: the host's writes go through the client's bounded command queue (`ClientConfig::cmd_queue_size`),
  so a program can't outrun its own link; nothing tells it whether the device keeps up. A device that can't has to drop,
  or tell the host with its own sideband events.
- **Credit sinks**: the device grants room in bytes and the host never sends more, see
  [Credit sinks](credit_sinks.md).

## Arrays of streams

Streams and sinks can be [arrays](arrays.md), and sit in arrays of traits:

```rust
#[ww_trait]
trait Board {
    #[counted]
    stream!(adc[]: [u8]);
    ww_impl!(port[]: Port);
}

#[ww_trait]
trait Port {
    stream!(event: PinEvent);
}
```

Each index is its own stream: the host opens `device.adc(2).call()` or `device.port(0).event().call()`, and every
device handler gets the index chain after `cx` (`index_chain: [UNib32; 1]`), as does `stream_adc(index_chain)`.
`valid_indices_root_adc()` says which indices exist. On the device, send through the same chain:
`stream_data_ser().adc_send(2, &mut state.adc_streams[2], &chunk, units, t, out)` for the array itself,
`stream_data_ser().port(0).event_send(&event, cx)` inside an array of traits.

Reliable streams can't be in arrays yet: `poll_streams` refuses them with a compile error.

## Properties as streams

An `ro` or `rw` property sends its changes as stream data on its own path:
`stream_data_ser().<property>_send(&value, out)` on the device, `observe_<property>()` on the host, a plain
`Stream<T>`. See [Observing properties](observing.md), superseded by `+observe` for properties that have it.

## More on streams

- [Counted and reliable streams](counted_streams.md): device state, producers, `PositionedStream`, resuming.
- [Stream delivery](stream_reliability.md): what counted and reliable streams guarantee, sessions, timestamps.
- [Stream stores](stream_stores.md): where a reliable stream keeps its data (RAM, SD card, flash).
- [Credit sinks and sideband](credit_sinks.md): flow control into the device, the sideband channel.
- [Stream protocol](stream_protocol.md): the bytes on the wire, acknowledgements, the `ww_stream` crate.
- [Batching](batching.md): a plain, counted or reliable stream of a `series` type can opt into sending samples in
  `Series` batches instead of one item per event.
