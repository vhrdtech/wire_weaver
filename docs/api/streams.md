# Streams

Two types of streams are supported - from server to client (`stream!`) and from client to server (`sink!`).
I.e., naming is from the perspective of the device (node) - stream out, sink in.

```rust
#[ww_trait]
trait MyDevice {
    stream!(byte: u8);
    sink!(word: u32);
    stream!(slice: Vec<u8>);
    sink!(user_defined: Vec<LedState>);
}
```

Any type supported by the SerDes system works with streams as well. Streams can be used for many things, e.g., sending
status updates or bytes from USART, frames to be transmitted on CAN bus, etc.

Stream writes are not acknowledged - the write message is sent out, and no response is awaited by a client, server
publishes a
stream update and similarly does not wait for any response from a client.
Sinks marked `#[credit]` get credit-based flow control instead, see
[below](#credit-based-flow-control-for-sinks); for other streams, backpressure can be built on the sideband channel.

Streams can have a beginning and an end, for example, to implement a file IO or firmware update, to deal with small
chunks
at a time and yet be able to signal a completion event. It is also possible to send a user-defined delimiter, to be
delivered in order with stream data, that can be used to implement frame synchronization.

Another useful property of streams is that they work on the object level. For the `slice` stream in the example above,
each individual array size is guaranteed to be preserved, even if multiple stream updates are transferred together at
transport level. Sending `[1, 2, 3]`, `[4]`, `[5, 6]` will result in the same arrays received on the other end, in the
same order.

## Sending stream updates

The server side generates `stream_data_ser()`, with `<name>_send(&value, out)` (async) and `<name>_send_blocking`
(sync) for every `stream!`, and `<name>(&value, scratch)` returning the serialized event. `out` is either the `cx`
a handler gets, or `server.sink()` anywhere else in the event loop:

```rust
// from a handler, goes out before its reply
server_impl::stream_data_ser().byte_send(&0xAA, cx).await?;
// from the event loop
server_impl::stream_data_ser().slice_send(&[1, 2, 3], &mut server.sink()).await?;
```

Streams inside trait arrays are reached the same way as on the client, e.g.,
`stream_data_ser().port(0).pin(7).event_send(&event, cx)`.

## Sideband channel

In order to facilitate stream open/close operations, frame synchronization and other operations, all streams
in addition to a data channel have:

* A sideband command channel
* A sideband event channel

All sideband data is sent in-order with the payloads (no reordering).

### Sideband commands

The following stream sideband commands are supported:

* Open
* Close
* FrameSync
* ChangeRate(ShaperConfig)
* SizeHint(u32)
* User(u32)
* CreditQuery { sent } (credit sinks only, see [below](#credit-based-flow-control-for-sinks))

All commands are optional, including open and close.
Depending on the application, it might be beneficial to start a stream automatically without waiting for the command.

See [StreamSidebandCommand](https://github.com/vhrdtech/ww_stdlib/blob/main/ww_client_server/src/lib.rs#:~:text=StreamSidebandCommand)

### Sideband events

The following stream sideband events are supported:

* Opened
* Closed
* FrameSync
* SizeHint(u32)
* User(u32)
* Credit { have, grant } and Resend { have, grant } (credit sinks only)

### Handling sideband on the device

A sideband command from the host calls `sideband_<name>(cx, sideband)` on the server. Returning `Some(sideband)` sends
it back as a sideband event on the same stream, `None` sends back only an acknowledgement. Either way the host gets an
answer and releases the request's seq right away; `open()`, `close()` and `sideband()` on the host don't wait for it.

The device can also send sideband events on its own, for streams and sinks alike: `stream_data_ser()` has
`<name>_sideband_send(sideband, out)`, `<name>_sideband_send_blocking` and `<name>_sideband(sideband, scratch)`, used
the same way as the data methods above:

```rust
// e.g., tell the host a sink's buffer is full
server_impl::stream_data_ser().word_sideband_send(StreamSideband::User(UNib32(1)), cx).await?;
```

On the host, a stream or sink receives everything the device sends after its constructor (e.g., `client.word().await`)
returns, events sent before that are not buffered. The first event is always `Connected` (or `Disconnected`), which
`recv()` skips. `Stream::recv_any()` returns sideband events along with data, `Sink::recv_sideband()` (`_blocking`,
`try_recv_sideband()`) returns the ones sent to a sink.

## Credit-based flow control for sinks

Plain sinks are not acknowledged: the host writes as fast as it can, and a device that can't keep up has to drop data
or tell the host to slow down with its own sideband events. A sink marked `#[credit]` gets flow control and
acknowledgements built in, for bulk transfers into a device over slow or lossy links: a firmware update plan streamed
into a single-bank MCU that stalls on every flash write, bytes for a UART bridge, frames for a CAN transmitter.

```rust
#[ww_trait]
trait Updater {
    /// Update plan: steps and data, tens to hundreds of KB
    #[credit]
    sink!(plan: [u8]);
}
```

The device grants the host **credit in elements**: one element is one value of the sink's type (one write), whatever
its type and size. The host may only send as many values as the device said it has slots for. The host waits (async),
blocks (blocking) or returns "would block" (`try_send`, for GUIs and promises) when it is out of credit, so it never
outruns the device's receive queue, however slow the device or fast the link is.

### Why elements and not bytes

A sink carries any type, and the device keeps what it received as values of that type, so it thinks in values:

* the device sizes its receive queue in element slots, for the worst-case element size: `slots = buffer bytes / max
  element size` (`SinkCredit::slots_for`). Generated `stream_data_ser().<sink>_max_element_bytes()` gives that size
  where it is known (`sized` types today; the bound of bounded types once SW-19 `MaxLen` lands, `None` until then,
  e.g., for `[u8]`), so the user decides the slot size from the API rather than guessing;
* positions count values, so a resend starts at a value boundary by construction, and the numbers stay small (a
  position costs 1 byte for the first 128 values of a session);
* the host never has to know how the device stores a value.

The price: a device that queues variable-size values in a byte buffer has to reserve the worst case per slot.

### Wire format

A credit sink is a different resource kind from a plain sink (`ApiItemKind::FlowSink { ty, flow: FlowControl::Credit
}` in `ww_self`), so introspection, the dynamic client and the Python client know to speak this protocol, and turning
a plain sink into a credit sink is an API break (its writes change).

Each write carries its **position**: the index of this value from the start of the session, followed by the value
itself, as `ww_client_server::CreditWrite { pos: UVlq32, data: TailBytes }` inside `RequestKind::Write`. That costs 1
byte for the first 128 values of a session, 2 bytes up to 16384, 3 bytes up to 2 M values.

Three `StreamSideband` variants are added (appended, so existing values keep their encoding):

| Sideband                   | Direction     | Meaning                                                                                       |
|----------------------------|---------------|-----------------------------------------------------------------------------------------------|
| `Credit { have, grant }`   | device → host | `have`: values accepted in order since `Open`; the host may send up to position `have + grant` |
| `CreditQuery { sent }`     | host → device | "I have sent up to `sent`, where are you?"; answered with `Credit` or `Resend`                 |
| `Resend { have, grant }`   | device → host | data after `have` was lost: send again from position `have`, up to `have + grant`              |

All three are cumulative: every one of them carries the full state, so losing one is harmless, the next one (or the
answer to the next query) replaces it. Positions are `u32`, a session is limited to `u32::MAX` values, `Open` starts
a new one.

### Session

1. The host sends `Open`. The device resets its counters and answers with `Credit { have: 0, grant: capacity }`
   (initial credit). Credit events left over from a previous session are ignored by the host until this one arrives.
   `send()` on a sink that was never opened opens it first.
2. The host sends values while `pos < have + grant`, keeping a copy of everything after `have` (at most the device's
   capacity in values) for a resend.
3. The device counts accepted values (`have`) as writes arrive, and the values it has **freed** (consumed, e.g.,
   written to flash). Its limit is `freed + capacity`. When the limit has moved by at least `threshold` values since
   the last grant, it sends a new `Credit`. A large threshold means few acks (a slow link), a small one keeps the pipe full.
4. When the host is out of credit and nothing arrives for `query_interval`, it sends `CreditQuery { sent }`; the
   device answers right away with its current state. This recovers from a lost `Credit`, from a device that freed
   less than `threshold`, and detects lost data at the end of a transfer.
5. `flush()` sends `CreditQuery` and waits until the device has accepted everything (`have == sent`): on a reliable
   link that is the one and only ack a transfer needs, when the device's capacity covers it. `close()` flushes,
   then sends `Close`.

### Lossy media: go-back-N from `have`

On UDP, LoRa or LTE-M a write can be lost. The device only accepts a write at `pos == have`:

* `pos < have`: a duplicate from a resend, dropped;
* `pos > have`: something before it was lost; the write is dropped and the device sends `Resend { have, grant }`,
  once per gap (the next in-order write re-arms it);
* a `CreditQuery { sent }` with `sent > have` means writes were lost after the last one that arrived (nothing came
  after them to reveal the gap), the device answers with `Resend` instead of `Credit`.

On `Resend` the host goes back to `have` and sends everything again from its copy. Requests are processed in order,
so when the device sees a `CreditQuery`, everything the host sent before it has either arrived or is lost. A plain
`Credit` never rewinds the host, even with `have < sent`: the rest may simply still be in flight.

### Device side

Generated server code does the bookkeeping, user code only says how many values it can queue and when it frees
some.
Every credit sink gets one more handler, returning the sink's state, `ww_client_server::credit::SinkCredit`, usually a
field of the server struct (arrays of sinks get the index chain, as other handlers do):

```rust
const CHUNK: usize = 256; // the host sends the plan in chunks of at most this many bytes
const SLOTS: u32 = SinkCredit::slots_for(4096, CHUNK as u32);

struct Server {
    plan_credit: SinkCredit, // SinkCredit::new(SLOTS), re-granted every SLOTS / 2 values by default
    plan_queue: heapless::Deque<heapless::Vec<u8, CHUNK>, { SLOTS as usize }>,
}

impl Server {
    fn credit_plan(&mut self) -> &mut SinkCredit {
        &mut self.plan_credit
    }

    fn write_plan(&mut self, cx: &mut Context<'_, impl BlockingEventOut>, data: &TailBytes<'_>) {
        // only called for writes in order, never more values than the credit granted
        self.plan_queue.push_back(heapless::Vec::from_slice(data.0).unwrap()).unwrap();
        if self.plan_queue.len() >= SLOTS as usize / 2 {
            let n = flash_write(&mut self.plan_queue); // stalls for a while, pops what it wrote
            self.plan_credit.free(n as u32);
        }
    }
}
```

* On `Open`, generated code calls `SinkCredit::open()`, then the user's `sideband_<name>` as usual (it may change the
  capacity there), then sends the initial `Credit`.
* `CreditQuery` is answered by generated code, the user's sideband handler doesn't see it.
* On a write, generated code checks the position, calls `write_<name>` for accepted writes only, and after it
  returns sends a `Credit` if `free()` moved the limit far enough.
* Data freed outside a handler (e.g., after a flash write in the event loop): `if let Some(sb) =
  credit.take_credit() { stream_data_ser().plan_sideband_send(sb, &mut server.sink()).await?; }`. If that is
  skipped, the host's next `CreditQuery` picks it up after `query_interval`.
* `SinkCredit::overruns()` counts values the host sent beyond its credit (should stay 0; such writes are still
  passed to the handler).

### Host side

`Sink<T>` of a credit sink works like any other one: `send()` (async), `send_blocking()`, plus `try_send()` that
returns `Ok(false)` without sending when there is no credit for the value yet, and `flush()` / `flush_blocking()`.
`Sink::set_credit_config(CreditConfig { query_interval, stall_timeout })` tunes it to the link: `query_interval`
(default 1 s) should be longer than the link's round trip, `stall_timeout` (default 30 s) is how long to wait without
any progress before `send()` fails with a timeout, longer than the device's worst stall (an H7 sector erase takes 1-2
s). `credit_progress()` returns `(accepted, sent)` values of the session, e.g., for a progress bar.

Related: [CLT-18 backpressure](../features.md#client-host-side-clt) (the command queue of the client),
`API-28` in the [features](../features.md) list.
