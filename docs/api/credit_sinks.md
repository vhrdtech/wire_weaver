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

**Positions count units**, which the host names for each item: one per value by default, the bytes of the payload
on a byte sink (`[u8]`, a UART's tx), or what `send_units(value, units)` says (at least one). So the device's
position, its `have`, means something to user code on both sides: bytes on the line, samples, segments. Credit stays
in bytes, positions in units.

**The host names the session**: every open is a `StreamOpen` with the host's session and position, a new random one
at 0 for `open()` and the first `send()`, or one user code picks with `open_session(session)` (from a hash of the data,
say). The device keeps a session it is in: an open of the same session (a repeated or late one, a reconnect)
changes nothing and answers where it is; any other starts at 0.

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
  (`with_threshold`), or a waiting host only learns of the room from its next `Query`.
- **The device consumes without waiting for more.** A device that collects data before writing it out (a flash page)
  has to write it once less room than the largest item is left: the host can't send the next item before that.

At the other end, a buffer is at most 64 MiB (`ww_stream::sink::MAX_WINDOW`): `sized_for` doesn't compile with a larger one,
`try_sized_for` and `set_capacity` return `SinkCreditError::BufferTooLarge`. That keeps positions on the wire short
and unambiguous ([positions wrap at 256 MiB](stream_protocol.md#positions-wrap-at-256-mib)); a session itself has no
size limit. The host also keeps at most `MAX_WINDOW` units in flight, and refuses an item of more.

### Device side

Generated server code does the bookkeeping; user code says how many bytes it can buffer and when it frees some. A
credit sink has one more handler, `credit_<name>()`, returning its state, a `ww_stream::sink::SinkCredit` (so the
firmware crate depends on `ww_stream`), usually a field of the server (arrays of sinks get the index chain and need
one state per index):

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
called with, what to `free()` once that value is consumed; `last_item_units()` its units as the host named them.

- On `StreamOpen`, generated code calls `SinkCredit::open(cursor)` (the host's session: kept if it is the one the
  device is in, else a new one at 0); for a new session it calls `sideband_<name>` with `Open` first, so user code
  sees where one logical write starts (it may change the capacity there; a resume of the same session doesn't call
  it). Then it answers with `StreamOpened` (where the device is, its buffer) and sends an `Ack` with the grant. An
  open without the host's session is refused.
- On a write (`SinkWrite`, `SinkWriteUnits`), generated code checks its position, calls `write_<name>` for writes in
  order only, and after it returns sends an `Ack` with new credit if `free()` moved the limit far enough. A write
  after a lost one asks for a `Resend` (once per gap); a write while the device has no session (it rebooted, or the
  host closed) answers `Close` once, and the host opens its session again.
- `Query` is answered by generated code, `sideband_<name>` doesn't see it; `Close` ends the session
  (`SinkCredit::close()`), then goes to `sideband_<name>` like any other sideband (it may free what is left there).
- A read on the sink (`<name>_status()` on the host) returns its `StreamStatus`: open or not, the session, the
  position, bytes buffered and the capacity.
- Data freed outside a handler (e.g. after a flash write in the event loop):
  `if let Some(sb) = state.plan_credit.take_ack()`, send it with
  `stream_data_ser().plan_sideband_send_blocking(sb, out)` (`plan_sideband_send` in an async server).
  If that is skipped, the host's next `Query` picks it up after its query interval.
- `SinkCredit::overruns()` counts bytes the host sent beyond its credit (should stay 0; such writes still reach the
  handler).

### Host side

`Sink<T>` of a credit sink works like any other: `send()` opens a session if needed and waits for credit. Besides:

- `open()` starts a new session and returns once the device answered; `open_session(session)` the same with the
  session named by user code.
- `send_units(value, units)` (`_blocking`, `try_send_units`) sends a value that takes `units` positions;
  `send_bytes()` on a byte sink counts its bytes.
- `flush()` / `flush_blocking()` / `try_flush()` waits until the device accepted everything sent so far, resending
  what was lost; `close()` flushes first, then sends `Close`.
- `credit_progress()` returns `(accepted, sent)` units of the session (`u64`, a session has no size limit), e.g. for
  a progress bar.
- A device that lost the session (it answers an open behind what it had acknowledged, after a reboot) fails the call
  with `StreamError::SinkSessionLost { device_at, acked }`: what was sent from `device_at` on is gone, user code
  decides what to do (a DFU plan starts over); the next value opens a new session. A link that went down without
  `keep_streams` fails the call with `UnexpectedEvent(Disconnected)` and ends the session on the host; to go on after
  a reconnect or a restart, see [Resume](#resume).
- `set_credit_config(CreditConfig { query_interval, stall_timeout })` tunes it to the link: `query_interval` (1 s by
  default) longer than the link's round trip, `stall_timeout` (30 s by default) longer than the device's worst stall
  (a flash sector erase takes 1-2 s); with no progress for that long, `send()` fails with a timeout.

### Resume

A credit sink's session lives on the device until another session is opened (or the device reboots), so a transfer
goes on after the link or the host program went away, from where the device is: nothing the device took is sent
again, nothing is lost.

**After a reconnect**, with the program still running: disconnect with `disconnect().keep_streams()`, and connect
again with `Commander::reconnect()` (the same transport and device selection, the same handles), which calls
`Commander::resume_streams()` (it also resumes counted and reliable streams and observers). That
registers every sink on the new connection and opens a credit sink's session again; the sink sends what the device
didn't acknowledge, from the device's position, and goes on. While the link is down, `send()` and `flush()` wait for
the resume (`try_send()` returns `false`); `resume_streams()` counts each session that goes on.

```rust
let mut tx = client.uart_tx().await?;
tx.send_bytes(&line).await?;
// .. the link goes
cmd.disconnect().keep_streams().await?;
// .. the device is back
cmd.reconnect().await?;
tx.send_bytes(&next_line).await?; // the device gets the rest of `line`, then `next_line`
```

**After a restart** of the host program, the program saves the sink's cursor with its own progress:
`Sink::cursor()` is the session and the position up to which the device accepted everything. The new program
continues with `<sink>_resume(cursor)` (`Sink::resume(cursor)` on a sink it has, `_blocking` variants), which
returns the sink and the position where the device is: at or after the saved one, since the device may have accepted
more before the old program went. User code continues its own data there, the byte at that offset on a byte sink:

```rust
// old program: from time to time, with what it sent
save(tx.cursor(), &state)?;
// new program
let (mut tx, at) = client.uart_tx_resume(saved_cursor).await?;
tx.send_bytes(&data[at as usize..]).await?;
```

**A device that lost the session** (it rebooted while the host was away) answers the open at 0: `resume()` and, after
a reconnect, the sink's next call fail with `StreamError::SinkSessionLost { device_at, acked }`, and
`resume_streams()` doesn't count the sink. What the device had is gone; user code decides (a DFU plan starts over
with `open()`, see [ww_dfu](../std_library/ww_dfu_ab.md#2c-app-bulk-swap-freight)).

A resume opens the same session (`StreamOpen` with the cursor), so a device's `sideband_<name>` doesn't see an `Open`:
for user code on the device, the logical write goes on. A late duplicate of an open (UDP) answers where the device is
and changes nothing. What can't be told apart is described in
[`ww_stream::sink`](stream_protocol.md#credit-sinks-on-the-wire).

The dynamic client and Python speak the same protocol for credit sinks (Python: `sink.cursor`,
`sink.resume(cursor)`, `ww.SessionLost`). How the credit is exchanged, and how
writes lost on UDP are found and resent: [Stream protocol](stream_protocol.md#credit-sinks-on-the-wire).

## Sideband

Next to its data, every stream and sink has a sideband channel in both directions, `StreamSideband` values sent in
order with the data. Generated code uses it for the protocol, user code may use the rest:

| Sideband | Used for |
|---|---|
| `Close` | the host closes a stream or sink (`close_<name>` on a stream, `sideband_<name>` on a sink); the device ends a stream |
| `Open` | the host opens a plain sink; a credit sink's handler gets it when a `StreamOpen` starts a new session |
| `FrameSync`, `SizeHint(n)`, `User(n)` | free for the API's own meaning: frame boundaries in a byte stream, the size of what follows, application events |
| `Ack`, `Resend`, `Query` | counted and reliable streams and credit sinks, handled by generated code |

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
