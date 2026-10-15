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

The kind is part of the API and visible in introspection, so it is chosen in the API crate. What a counted or
reliable stream guarantees, and how, is the subject of [Stream delivery](stream_reliability.md); this page shows how
to declare, implement and use every kind.

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

On the wire, opening is a `RequestKind::StreamOpen { resume, args }` request, answered with a
`ww_client_server::stream::StreamOpened` (the session and positions, empty for a plain stream) followed by the
function's result. Closing is a `StreamSideband::Close` request from the host, or a `Close` event from the device
when the stream ends on its own.

## Device side

`ww api scaffold <api_crate>` prints every handler and state field a server needs, with the right signatures for
the `ww_codegen!` options (see [`ww api`](../cli.md#ww-api)). For a stream named `samples` they are:

| Handler | Plain | Counted | Reliable | Called |
|---|---|---|---|---|
| `fn samples(&mut self, cx, args..) -> RpcResult<R>` | ✓ | ✓ | ✓ | the host opens the stream |
| `fn close_samples(&mut self, cx)` | ✓ | ✓ | ✓ | the host closes it |
| `fn sideband_samples(&mut self, cx, sideband) -> Option<StreamSideband>` | ✓ | | | other sideband from the host, see [Sideband](#sideband) |
| `fn stream_samples(&mut self) -> &mut Counted` / `&mut Reliable<S>` | | ✓ | ✓ | whenever generated code needs the stream's state |
| `fn new_stream_session(&mut self) -> u32` (one per server) | | ✓ | ✓ | a host opens a counted or reliable stream |

All handlers get `cx: &mut Context<'_, impl EventOut, Medium>` (`impl BlockingEventOut` for `use_async = false`);
streams in [arrays](#arrays-of-streams) get their index chain after it. The open handler answers right away:
`Ready(..)` with the result, `Unimplemented` refuses it; a deferred answer is not supported for an open.

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

### Counted streams

A counted stream's state is a `ww_stream::tx::Counted`, usually a field of the server, handed to generated code by
`stream_<name>()`. Generated code opens it (a new session, numbered by `new_stream_session()`), resumes it, closes it
and answers a read on the stream with its `StreamStatus`. Producers send through it, naming the units of each item and
the device time of its first unit, if any:

```rust
struct ServerState {
    rx_stream: Counted,
    sessions: u32,
}

impl ServerState {
    async fn rx(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }
    async fn close_rx(&mut self, _cx: &mut Context<'_, impl EventOut>) {}
    async fn stream_rx(&mut self) -> &mut Counted {
        &mut self.rx_stream
    }
    /// Put a boot counter into the high bits, so that a cursor from before a reboot never matches a new session.
    fn new_stream_session(&mut self) -> u32 {
        self.sessions = self.sessions.wrapping_add(1);
        self.sessions
    }
}

let ser = server_impl::stream_data_ser();
// sent right away; while no host has the stream open, the units are counted as lost instead
ser.rx_send(&mut state.rx_stream, &chunk, chunk.len() as u64, Some(t), &mut server.sink()).await?;
// the producer's own buffer overflowed: the next event reports it
state.rx_stream.lost(dropped_bytes);
```

`stream_<name>()` may return anything that dereferences to the state, e.g. a mutex guard when a producer task shares
it. Counted streams work in sync servers too (`<name>_send_blocking`).

### Reliable streams

A reliable stream's state is a `ww_stream::tx::Reliable<S>` with a store `S` that keeps every event until the host
acknowledges it: `RamRing<N>` in RAM, `PersistentRamRing<N>` across a soft reset, or `LogStore` on an SD card or flash
(see [Stream stores](stream_stores.md)). Stores are async, so reliable streams need an async server
(`use_async = true`, a compile error otherwise).

Producers push into the store and never wait for the link; the generated `poll_streams` sends what the stores have,
called from the event loop between requests:

```rust
struct ServerState {
    samples_stream: Reliable<RamRing<{ 16 * 1024 }>>,
    sessions: u32,
}

impl ServerState {
    async fn stream_samples(&mut self) -> &mut Reliable<RamRing<{ 16 * 1024 }>> {
        &mut self.samples_stream
    }
    // samples(), close_samples() and new_stream_session() as above
}

// producer: `scratch` holds the serialized item on its way into the store
let pushed = server_impl::stream_data_ser()
    .samples_push(&mut state.samples_stream, &block, 256, Some(t), &mut scratch)
    .await?; // Pushed::Lost when the store was full: counted, reported to the host

// event loop
loop {
    if let Either::First(ready) = select(server.wait(), Timer::after_millis(5)).await
        && let Some(event) = server.handle(ready, state).await
        && matches!(event, LinkEvent::Down(_))
    {
        // stop sending, keep the data for the host's resume
        state.samples_stream.detach();
    }
    if server.is_up() {
        let now_ms = Instant::now().as_millis();
        // by priority, live before backlog, up to 2048 bytes of items; `buf` holds the biggest item
        _ = state.poll_streams(&mut server.sink(), now_ms, &mut buf, 2048).await;
    }
}
```

`poll_streams(out, now_ms, buf, budget_bytes)` is generated when the API has reliable streams. It also asks a host
that went silent where it is, and the host's acknowledgements and resend requests reach the store through the
dispatcher. Detaching on `LinkEvent::Down` is the firmware's job: a detached stream sends nothing and keeps its data
for the host's resume.

A reliable stream only stores once it has a session: from the first open by a host, or from boot on when the
firmware starts one itself, so that a logger keeps what it records before any host connects:

```rust
// at boot, before the event loop
let session = state.new_stream_session();
state.samples_stream.start_session(session).await?;
// or, with a store that survives reboots: continue the session it holds
state.samples_stream.continue_stored_session();
```

`Reliable::new(store).priority(Priority::High)` sets the stream's priority against other reliable streams, see
[Sending: producers, the event loop and priorities](stream_reliability.md#sending-producers-the-event-loop-and-priorities).
`examples_mcu/rp2/src/bin/usb_log.rs` is a complete firmware with a reliable log stream.

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

### Counted and reliable streams: `PositionedStream<T>`

```rust
while let Ok(ev) = samples.recv_event().await {
    match ev {
        StreamEvent::Item { pos, units, t, run, value, .. } => { /* run: Live or Backfill */ }
        StreamEvent::Gap { at, units, cause, .. } => { /* cause: Device, Link, Store, Host */ }
        StreamEvent::Resumed { cursor, ended, .. } => { /* ended: the old session, when the device has a new one */ }
        StreamEvent::BackfillDone | StreamEvent::Disconnected | StreamEvent::Closed => {}
    }
}
```

- `recv()` (`_blocking`, `try_`) returns values only; gaps are logged and counted in `stats()` (`StreamStats`:
  received, lost by cause, duplicates, late, out of order, resend requests, queued).
- `cursor()` is where to resume from: everything before it was received (counted) or acknowledged (reliable). A
  recorder saves it with its data. `session()` is the device's session number.
- `.ack_mode(AckMode::Explicit)` when opening: a reliable stream's data counts as received only once `ack(pos)`
  confirms it, e.g. after writing it to disk, so a crash in between loses nothing. By default (`AckMode::OnReceive`)
  it counts when user code takes it.
- `split()` gives one handle per run, each in order: a dashboard reads the live one, a recorder both.
- `close()`, or dropping the last handle, closes the stream on the device.

The receiver runs in the client's event loop, so a program that doesn't read still answers the device, and its memory
stays bounded: a reliable stream holds at most its window (`.window(bytes)`, 4 MiB by default) and the device keeps
the rest in its store; a counted stream drops what goes beyond its queue and reports it as `Gap { cause: Host }`.

### Resuming

Counted and reliable streams also get `<name>_resume(cursor)` and `<name>_status()`:

```rust
// after a restart: the backlog from the saved cursor, beside live data
let mut samples = device.samples_resume(saved_cursor).call().await?;
// open or not, session, positions, store use
let status = device.samples_status().read().await?;
```

A resume continues the session with the arguments it was opened with on the device, so it has no arguments and no
result. A cursor of an older session (the device opened the stream again or lost its store) starts with the current
session, and `Resumed { ended: Some(old) }` says so. Across a reconnect of the same program,
`disconnect().keep_streams()` keeps the handles open (they see `Disconnected`, not the end), and
`Commander::resume_streams()` on the next connection resumes each from its cursor: the same handle goes on with
`Resumed` and whatever the device kept meanwhile.

The dynamic client opens streams with `DynResource::positioned_stream` (`DynPositionedStream`, values as
`ValueOwned`), Python with `open()` on the stream ([Python](../python.md#counted-and-reliable-streams)).

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
  [Acknowledgements and resends](stream_reliability.md#acknowledgements-and-resends)).
- **Plain sinks**: the host's writes go through the client's bounded command queue (`ClientConfig::cmd_queue_size`),
  so a program can't outrun its own link; nothing tells it whether the device keeps up. A device that can't has to drop,
  or tell the host with its own sideband events.
- **Credit sinks**: the device grants room in bytes and the host never sends more, below.

## Credit-based flow control for sinks

A sink marked `#[credit]` gets flow control and acknowledgements built in, for bulk transfers into a device over slow
or lossy links: a firmware update plan streamed into a single-bank MCU that stalls on every flash write, bytes for a
UART bridge, frames for a CAN transmitter.

```rust
#[ww_trait]
trait Updater {
    /// Update plan: steps and data, tens to hundreds of KB, in pieces of up to 1 KiB
    #[credit]
    sink!(plan: MaxLen<RefVec<'i, u8>, 1024>);
}
```

The device grants the host **credit in bytes**: the bytes of the sink's items, one item being the serialized value of
one write, as sent. The host sends an item only when all of it fits into the credit left, and never splits one. Out of
credit, `send()` waits, `send_blocking()` blocks and `try_send()` returns `Ok(false)`, so the host never outruns the
device's receive buffer, however slow the device or fast the link.

### Why bytes and not values

What runs out on a device is RAM: the bytes of its receive buffer. Counting values (one slot per write) would make
the device size every slot for the largest value, so a buffer that holds sixteen 1 KiB pieces would also hold only
sixteen 20-byte ones. Counting bytes, the same buffer holds as many items as fit, however their sizes vary.

The price is one rule, since items are never split: **the device must always be able to grant at least its largest
item**, or the host could never send it. Three things keep a transfer from getting stuck:

- **The buffer holds the largest item.** The generated `<SINK>_MAX_ITEM_BYTES: Option<u32>` on the level's stream
  serializer (also `stream_data_ser().<sink>_max_item_bytes()`, a `const fn`) is the largest item, from the type's
  `MAX_SIZE`: `MaxLen<RefVec<'i, u8>, N>` gives `N` (bytes are sent as is), a type without an upper bound (`[u8]`)
  `None`. `SinkCredit::sized_for(buffer_bytes, max_item)` checks it, a compile error in a `const`;
  `try_sized_for()` returns an error for a size known at runtime. For an unbounded type the host checks: a value
  bigger than the device's buffer (the first credit of a session) fails `send()` with `StreamError::ItemTooBig`
  instead of waiting forever.
- **A re-grant lets the largest item in.** The device re-grants once it freed `threshold` bytes (half the buffer by
  default); `sized_for` caps the threshold at `buffer - largest item + 1`, so a host waiting for room for a big item
  always gets it with the next grant. With an unbounded type, set the threshold that way yourself
  (`with_threshold`), or a waiting host only learns of the room from its next `CreditQuery`.
- **The device consumes without waiting for more.** A device that collects data before writing it out (a flash page)
  has to write it once less room than the largest item is left: the host can't send the next item before that.

### Device side

Generated server code does the bookkeeping; user code says how many bytes it can buffer and when it frees some. A
credit sink has one more handler, `credit_<name>()`, returning its state, a `ww_client_server::credit::SinkCredit`,
usually a field of the server (arrays of sinks get the index chain and need one state per index):

```rust
const BUF: usize = 4096;
/// Some(1024), from the API's MaxLen bound
const LARGEST: Option<u32> = server_impl::stream_data_ser().plan_max_item_bytes();
/// a buffer smaller than the largest item would not compile
const PLAN_CREDIT: SinkCredit = SinkCredit::sized_for(BUF as u32, LARGEST);

struct Server {
    plan_credit: SinkCredit, // PLAN_CREDIT, re-granted every BUF / 2 bytes freed
    plan_buf: heapless::Vec<u8, BUF>,
}

impl Server {
    fn credit_plan(&mut self) -> &mut SinkCredit {
        &mut self.plan_credit
    }

    fn write_plan(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, data: &TailBytes<'_>) {
        // only called for writes in order, never more bytes than the credit granted
        self.plan_buf.extend_from_slice(data.0).unwrap();
        // the next item might not fit: write out now, the host can't send more before that
        if self.plan_buf.len() > BUF - LARGEST.unwrap() as usize {
            flash_write(&self.plan_buf); // stalls for a while
            self.plan_credit.free(self.plan_buf.len() as u32);
            self.plan_buf.clear();
        }
    }

    fn sideband_plan(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: StreamSideband,
    ) -> Option<StreamSideband> {
        None
    }
}
```

A handler of a typed sink gets the value, not its bytes: `SinkCredit::last_item_len()` is the size of the item it was
called with, what to `free()` once that value is consumed.

- On `Open`, generated code calls `SinkCredit::open()` (a new session), then `sideband_<name>` as for any sideband
  (it may change the capacity there), then sends the initial credit.
- On a write, generated code checks its position, calls `write_<name>` for writes in order only, and after it
  returns sends new credit if `free()` moved the limit far enough.
- `CreditQuery` is answered by generated code, `sideband_<name>` doesn't see it.
- Data freed outside a handler (e.g. after a flash write in the event loop):
  `if let Some(sb) = state.plan_credit.take_credit()`, send it with
  `stream_data_ser().plan_sideband_send_blocking(sb, out)` (`plan_sideband_send` in an async server).
  If that is skipped, the host's next `CreditQuery` picks it up after its query interval.
- `SinkCredit::overruns()` counts bytes the host sent beyond its credit (should stay 0; such writes still reach the
  handler).

### Host side

`Sink<T>` of a credit sink works like any other: `send()` opens the session if needed and waits for credit. Besides:

- `flush()` / `flush_blocking()` / `try_flush()` waits until the device accepted everything sent so far, resending
  what was lost; `close()` flushes first, then sends `Close`.
- `credit_progress()` returns `(accepted, sent)` bytes of the session, e.g. for a progress bar.
- `set_credit_config(CreditConfig { query_interval, stall_timeout })` tunes it to the link: `query_interval` (1 s by
  default) longer than the link's round trip, `stall_timeout` (30 s by default) longer than the device's worst stall
  (a flash sector erase takes 1-2 s); with no progress for that long, `send()` fails with a timeout.

The dynamic client and Python speak the same protocol for credit sinks.

### On the wire

A credit sink is its own resource kind (`ApiItemKind::FlowSink { ty, flow: FlowControl::Credit }` in `ww_self`), so
turning a plain sink into a credit sink is an API break: its writes change. Each write is a
`ww_client_server::CreditWrite { pos: UVlq32, data: TailBytes }` inside `RequestKind::Write`, `pos` being the byte
position of the item's first byte since the session's `Open` (1 byte for the first 128 bytes of a session, 2 up to
16 KiB, 3 up to 2 MiB, 4 up to 256 MiB). Three sideband messages carry the rest:

| Sideband | Direction | Meaning |
|---|---|---|
| `Credit { have, grant }` | device → host | `have` bytes of items accepted in order; the host may send up to byte `have + grant` |
| `CreditQuery { sent }` | host → device | "I have sent up to byte `sent`, where are you?", answered with `Credit` or `Resend` |
| `Resend { have, grant }` | device → host | writes after `have` were lost: send again from byte `have` (an item boundary), up to `have + grant` |

All three carry the whole state, so losing one is harmless: the next one, or the answer to the next query, replaces
it. Positions are 32 bit, a session is limited to 4 GiB.

1. `Open` starts a session: the device resets its counters and answers with `Credit { have: 0, grant: capacity }`.
   Credit events left over from an earlier session are ignored by the host until this one arrives.
2. The host sends an item while `pos + its length <= have + grant`, keeping a copy of everything after `have` for a
   resend (at most the device's capacity).
3. The device counts accepted bytes (`have`) and bytes it **freed** (consumed, e.g. written to flash); its limit is
   `freed + capacity`. When the limit has moved by at least `threshold` since the last grant, it sends a new
   `Credit`: a large threshold means few acknowledgements over a slow link, a small one keeps the link full.
4. When the host is out of credit and nothing arrives for `query_interval`, it sends `CreditQuery`. This recovers from a
   lost `Credit`, from a device that freed less than `threshold`, and finds lost data at the end of a transfer.
5. `flush()` sends `CreditQuery` and waits until `have == sent`: on a reliable link with a buffer that holds the whole
   transfer, the one acknowledgement it needs.

On a lossy medium (UDP, LoRa, LTE-M) the device accepts a write only at `pos == have`: one before it is a duplicate
from a resend and is dropped; one after it means something was lost, so it is dropped too and the device sends
`Resend { have, grant }`, once per gap. A `CreditQuery { sent }` with `sent > have` means writes were lost after the
last one that arrived; the device answers with `Resend` instead of `Credit`. On `Resend` the host goes back to `have`
and sends everything again from its copy (go-back-N). Requests are processed in order, so when the device sees a
`CreditQuery`, everything sent before it has arrived or is lost; a plain `Credit` never rewinds the host, the rest may
still be in flight. An empty item takes one position, so its loss is noticed like any other.

Credit sinks are planned to move onto the positions, acknowledgements and resume of counted and reliable streams
(`API-34` in [Features](../features.md)).

## Sideband

Next to its data, every stream and sink has a sideband channel in both directions, `StreamSideband` values sent in
order with the data. Generated code uses it for the protocol, user code may use the rest:

| Sideband | Used for |
|---|---|
| `Close` | the host closes a stream or sink (`close_<name>` on a stream, `sideband_<name>` on a sink); the device ends a stream |
| `Open` | the host opens a sink (credit sinks: a new session) |
| `FrameSync`, `SizeHint(n)`, `User(n)` | free for the API's own meaning: frame boundaries in a byte stream, the size of what follows, application events |
| `Credit`, `CreditQuery`, `Resend` | credit sinks, handled by generated code |
| `Ack`, `ResendFrom`, `Query` | counted and reliable streams, handled by generated code |

From the host, plain streams and sinks pass a sideband to the device's `sideband_<name>(cx, sideband)`: returning
`Some(sideband)` sends it back as a sideband event on the same stream, `None` only acknowledges the request. Either way
the host's request gets its answer. Counted and reliable streams take only `Close` and the protocol messages, other
sideband is refused.

```rust
// host
stream.sideband(StreamSideband::User(UNib32(5))).await?;
match stream.recv_any().await? {
    TypedStreamEvent::Sideband(StreamSideband::User(n)) => { /* .. */ }
    _ => {}
}
// device
fn sideband_uart_rx(
    &mut self,
    _cx: &mut Context<'_, impl BlockingEventOut>,
    sideband: StreamSideband,
) -> Option<StreamSideband> {
    Some(sideband) // echo
}
```

The device sends sideband events on its own with `stream_data_ser().<name>_sideband_send(sideband, out)`
(`_sideband_send_blocking`, `<name>_sideband(sideband, scratch)`), for streams and sinks alike; a `Close` this way ends
a stream on the host (`recv_all_bytes()` returns, a `PositionedStream` sees `Closed`):

```rust
// e.g. tell the host a sink's buffer is full
server_impl::stream_data_ser().uart_tx_sideband_send(StreamSideband::User(UNib32(1)), cx).await?;
```

On the host, a `Stream` returns sideband events from `recv_any()`, a `Sink` from `recv_sideband()` (`_blocking`,
`try_recv_sideband()`); credit events of a credit sink are used up internally.

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
`Stream<T>`. See [Observing properties](../api/observing.md), superseded by `+observe` for properties that have it.
