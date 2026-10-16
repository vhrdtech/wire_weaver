---
level: nomicon
---

# Credit sinks and sideband

A plain sink ([Streams](streams.md#sinks)) sends what the host writes and hopes the device keeps up. A **credit
sink** lets the device say how much it can take, and resends what a lossy link lost. Every stream and sink also has
a **sideband** channel next to its data, for open and close and for the API's own signals.

## Credit sinks

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

The dynamic client and Python speak the same protocol for credit sinks. How the credit is exchanged, and how
writes lost on UDP are found and resent: [Stream protocol](stream_protocol.md#credit-sinks-on-the-wire).

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
