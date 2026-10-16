---
level: nomicon
---

# Counted and reliable streams

A counted stream gives every item a position, so the host learns exactly what it missed; a reliable stream also
keeps its items in a store on the device until the host acknowledges them, and sends them again after a loss or a
reconnect. Both are declared in the API with `#[counted]` / `#[reliable]` (see [Streams](streams.md)); what they
guarantee and how is in [Stream delivery](stream_reliability.md). This page shows the device's and the host's code.

```rust
#[ww_trait]
trait Adc {
    /// Sample sets at `config.rate`; unit: sample set
    #[stream(SampleBlock<'i>, reliable)]
    fn samples(config: StreamConfig) -> Result<StreamActual, AdcError<'i>>;

    /// Bytes as they arrive on the line; unit: byte
    #[counted]
    stream!(rx: [u8]);
}
```

## Device side

Besides the open and close handlers every stream has, a counted or reliable stream needs `stream_<name>()`, returning
its state, and the server one `new_stream_session()` (see the handler table in
[Streams](streams.md#device-side)).

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
[Sending: producers, the event loop and priorities](stream_protocol.md#sending-producers-the-event-loop-and-priorities).
`examples_mcu/rp2/src/bin/usb_log.rs` is a complete firmware with a reliable log stream.

## Host side: `PositionedStream<T>`

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

## Resuming

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
`ValueOwned`), Python with `open()` on the stream ([Python](../python_advanced.md#counted-and-reliable-streams)).
