# ww_can_bus

Part of the [IO trait family](io_family.md). `ww_can_bus` is a WireWeaver trait for a CAN controller with its
transceiver: a USB-CAN adapter, a gateway, a node with a CAN port, a bus analyzer. The host configures the bus,
sends and receives classic and CAN FD frames, and watches the controller's error state. It is `no_std` by default and
depends on `wire_weaver` and `ww_si` (`Baud` bit rates, `Hertz` for the controller clock, `Ohm` for terminators and
`Second` timestamps).

```toml
[dependencies]
ww_can_bus = "0.1"
```

## One trait, optional items

`Can` is one controller. A device with several puts them into an [array](../api/arrays.md):
`ww_impl!(can[]: ww_can_bus::Can)`. Six items are required, and they are everything a simple node needs; the rest
are [optional items](../api/optional_items.md) a device leaves out when its hardware doesn't have them:

```rust
#[ww_trait]
pub trait Can {
    fn capabilities() -> Capabilities<'i>;
    fn configure(config: CanConfig) -> Result<CanConfig, CanError<'i>>;
    fn config() -> CanConfig;
    property!(ro status: Status);
    #[counted]
    stream!(rx: RxFrame<'i>);
    #[credit]
    sink!(tx: TxFrame<'i>);

    #[optional]
    fn recover() -> Result<(), CanError<'i>>;
    #[optional]
    fn tx_abort() -> u32;
    #[optional]
    #[counted]
    stream!(tx_events: TxEvent);
    #[optional]
    #[counted]
    stream!(bus_errors: BusError);
    #[optional(filters)]
    fn set_filters(filters: FilterConfig<'i>) -> Result<(), CanError<'i>>;
    #[optional(rx_buffers)]
    fn rx_buffer(index: u16) -> Result<Option<BufferedFrame<'i>>, CanError<'i>>;
    #[optional(tx_buffers)]
    fn tx_buffer(index: u16, frame: Option<TxFrame<'i>>, period: Option<Second>) -> Result<(), CanError<'i>>;
    #[optional(terminator)]
    fn set_termination(termination: Termination) -> Result<Termination, CanError<'i>>;
    #[optional(terminator)]
    property!(ro termination: Termination);
    #[optional]
    fn test_pins(tx: TxPin) -> Result<BusLevel, CanError<'i>>;
}
```

| Item | Required | What it is for |
|---|---|---|
| `capabilities()` | ✓ | Controller clock, bit timing limits, CAN FD and TDC, which modes and options work, FIFO / buffer / filter counts, how frames are timestamped, terminator values |
| `configure(CanConfig)` | ✓ | Mode, nominal and data bit timing, TDC, options and bus-off recovery in one call, answered with what was set |
| `config()` | ✓ | The configuration in effect, for a host attaching to a running node |
| `status` | ✓ | Error state, TEC / REC and the last protocol error; observable, so a bus-off reaches the host without polling |
| `rx` | ✓ | Received frames, a counted stream (unit: frame) with the receive time in the head |
| `tx` | ✓ | Frames to send, a credit sink: the host never overruns the device's TX queue |
| `recover()` | | Start the bus-off recovery on request (devices that only recover by themselves leave it out) |
| `tx_abort()` | | Drop every frame queued and not on the bus yet |
| `tx_events` | | When each sent frame went on the bus, with its marker, or why it didn't |
| `bus_errors` | | Every error detected on the bus: kind, phase, direction |
| `set_filters` (`filters`) | | Acceptance filters: exact ID, mask or range, into a FIFO, a dedicated buffer, or rejected |
| `rx_buffer` (`rx_buffers`) | | The latest frame of a dedicated RX buffer, without streaming every frame |
| `tx_buffer` (`tx_buffers`) | | A dedicated TX buffer: send once, or cyclically every `period` by the device |
| `set_termination`, `termination` (`terminator`) | | A switchable terminator (relay, MOSFET) |
| `test_pins` | | Production test: drive TX, read RX |

A simple node leaves all of the optional ones out:

```rust
ww_codegen!(
    my_api::Node for Server,
    server = true,
    omit = [
        "can.recover", "can.tx_abort", "can.tx_events", "can.bus_errors", "can.filters",
        "can.rx_buffers", "can.tx_buffers", "can.terminator", "can.test_pins"
    ],
);
```

Why one trait: on CAN, watching the bus is a mode of the same controller (`Mode::ListenOnly`, `Mode::Restricted`)
with the same receive path, unlike I2C there is no sniffer without a controller, so an analyzer is `Can` with no
filters and the `bus_errors` stream. Controllers sharing a clock or message RAM (STM32 FDCAN instances) don't need a
group trait either: the host asks for a bit rate and gets back what was set, the clock is reported in
`Capabilities::clock`, and an instance that can't be changed alone answers `Common(InUse)`. Each optional item
appears on a different set of controllers, so they don't form one "advanced" trait:

| | bxCAN | M_CAN (STM32 FDCAN) | MCP2518FD | ESP32 TWAI | SocketCAN | gs_usb / candleLight |
|---|---|---|---|---|---|---|
| CAN FD | – | ✓ | ✓ | – | ✓ | ✓ |
| `tx_events` | – | TX event FIFO | TEF | – | own-message echo | echo frames |
| `bus_errors` | LEC | PEA / PED, LEC | diagnostics | error capture | error frames | BERR |
| `set_filters` | filter banks | std / ext elements | 32 filters | one mask | socket filters | – |
| `rx_buffer` | – | dedicated RX buffers (full M_CAN) | FIFO of depth 1 | – | – | – |
| `tx_buffer` | mailboxes | dedicated TX buffers | FIFOs | – | BCM | – |
| `recover` | without ABOM | INIT | – | `initiate_recovery` | restart | reset |
| terminator | board | board | board | board | `termination` | TERMINATION |

## Frames

```rust
pub struct Frame<'i> {
    pub id: CanId,                                  // Standard(U11) or Extended(U29)
    pub kind: FrameKind,                            // Data, Remote { len }, Fd { brs, esi }
    pub data: MaxLen<RefVec<'i, u8>, MAX_DATA_LEN>, // up to 64 bytes
}
```

`CanId::standard(0x123)` and `CanId::extended(0x18FF_0001)` build an ID (`None` when out of range). With the
`embedded-can` feature, `CanId` converts from and to `embedded_can::Id`, and on hosts `FrameOwned` implements
`embedded_can::Frame` (classic and remote frames; embedded-can has no FD frames).

A received frame is an `RxFrame { frame, filter }`, `filter` being the index of the filter element that accepted it
(`None` without filters or for non-matching frames). A frame to send is a `TxFrame { frame, marker }`: the marker
comes back in the frame's `TxEvent`, so a host matches events to what it sent (M_CAN's message marker, MCP2518FD's
sequence number, gs_usb's echo ID). On the wire a classic standard frame with 8 bytes takes 12 bytes as an
`RxFrame` or `TxFrame`, a 64-byte FD frame with a marker 69.

CAN XL frames are not part of `Frame`: a credit sink sizes the device buffer for its largest item, and frames of up
to 2048 bytes would make every classic node reserve that much. XL will come as its own optional sink and stream.

## Configuration

```rust
pub struct CanConfig {
    pub mode: Mode,                       // Off, Normal, Restricted, ListenOnly, Internal/ExternalLoopback, Test
    pub nominal: Timing,                  // arbitration phase
    pub fd: Option<FdConfig>,             // data phase timing, TDC, ISO / non-ISO; None: classic only
    pub options: Options,                 // one_shot, presume_ack, triple_sample, tx_pause, tx_order, rx_overflow
    pub bus_off_recovery: Option<Second>, // recover by itself after this long; None: wait for `recover`
}

pub enum Timing {
    Rate { rate: Baud, sample_point: u16 }, // sample point in tenths of a percent
    Segments(BitTiming),                    // prescaler, seg1, seg2, sjw in time quanta
}
```

A host asks for a bit rate and sample point, or for exact segments; `configure` always answers with segments, so the
host sees what the controller runs at. `BitTiming` counts like SocketCAN and the M_CAN `NBTP` register: a bit is
`1 + seg1 + seg2` quanta of `prescaler` clock periods, sampled after `1 + seg1`, so
`BitTiming::bit_rate(clock_hz)` and `BitTiming::sample_point()` follow from the answer and
`Capabilities::clock`. `Capabilities::nominal` / `data` give the segment limits (as SocketCAN's
`can_bittiming_const`), for a host that computes timings itself.

`Tdc::Auto` leaves the transceiver delay compensation to the device (offset at the data sample point), and the answer
says the offset it set. A setting the controller doesn't have is `Common(Unsupported)`, a bit rate it can't reach
`Common(InvalidArgument)`.

```rust
let actual = node
    .can()
    .configure(CanConfig {
        mode: Mode::Normal,
        nominal: Timing::Rate { rate: Baud::kilobaud(500u16), sample_point: 875 },
        fd: Some(FdConfig {
            data: Timing::Rate { rate: Baud::megabaud(2u8), sample_point: 750 },
            tdc: Tdc::Auto,
            iso: true,
        }),
        options: Options {
            one_shot: false,
            presume_ack: false,
            triple_sample: false,
            tx_pause: false,
            tx_order: TxOrder::Fifo,
            rx_overflow: RxOverflow::KeepOldest,
        },
        bus_off_recovery: Some(Second::milliseconds(100u8)),
    })
    .call()
    .await?;
let nominal = actual.nominal.segments().unwrap(); // e.g. prescaler 1, seg1 139, seg2 20 at 80 MHz
```

## Sending and receiving

```rust
let mut rx = node.can().rx().call().await?;
let mut tx = node.can().tx().await?;
tx.send_owned(TxFrameOwned { frame, marker: Some(UNib32(1)) }).await?;
while let Ok(event) = rx.recv_event().await {
    if let StreamEvent::Item { t, value, .. } = event {
        println!("{t:?} {:?} {:02x?}", value.frame.id, value.frame.data);
    }
}
```

`rx` is a counted stream: every event head carries the frame's position, the frames the device lost right before
it (a full receive FIFO, a full link buffer) and the time it was received. `tx` is a credit sink in bytes of
serialized frames; a frame the controller can't send as configured (FD with FD off, anything in a listen-only mode)
is dropped and reported as `TxOutcome::Rejected` on `tx_events`.

## Time

Received frames, TX events and bus errors carry their time in the event head: a `Second` of the device's one clock
(`ww_timebase::Clock`), in the prefix and number type the device picks (usually a `u32` of µs, which wraps every
71.6 minutes; the host unwraps it around `Clock::now()`). `Capabilities::timestamps` says how fine it is and where in
the frame it was taken: start of frame (M_CAN, bxCAN in TTCM, MCP2518FD), end of frame (gs_usb), or by software when
the frame was read out (no hardware timestamps); `None`, and heads without a time, on devices without a clock.

## Status and errors

`status` is an observable read-only [property](../api/properties.md): `Status { state, tec, rec, last_error,
last_data_error }`, `state` being `Stopped`, `Active`, `Warning`, `Passive` or `BusOff`. The device sends it to
observers when the state changes:

```rust
let mut status = node.can().observe_status().await?;
while let Ok(s) = status.recv().await {
    if s.state == ErrorState::BusOff { node.can().recover().call().await?; }
}
```

Methods return `CanError`: [`CommonError`](../api/errors.md#common-errors) as `Common` (unsupported, invalid
argument, in use, wrong state, ...), plus `OutOfFilters { max }` (fall back to fewer or wider filters) and `BusOff`
(recover first). Each method lists what it returns with `#[errors(..)]`. Errors on the bus are not errors of a call:
they are counted in `Status` and reported one by one on `bus_errors`.

## Filters and dedicated buffers

```rust
node.can().set_filters(FilterConfigOwned {
    filters: MaxLen::new(vec![
        Filter { matching: FilterMatch::Exact(CanId::standard(0x100).unwrap()), action: FilterAction::Buffer(2) },
        Filter {
            matching: FilterMatch::Range { from: CanId::standard(0x200).unwrap(), to: CanId::standard(0x20F).unwrap() },
            action: FilterAction::Fifo(0),
        },
    ])?,
    non_matching_standard: NonMatching::Reject,
    non_matching_extended: NonMatching::Accept { fifo: 1 },
    reject_remote: false,
}).call().await?;
let latest = node.can().rx_buffer(2).call().await?; // the last 0x100 frame, if one came
```

The first matching filter wins. A device maps the list to its hardware (filter banks, elements, masks) and answers
`OutOfFilters` or `Unsupported` (a range on a mask-only controller) when it can't. `tx_buffer(index, Some(frame),
Some(period))` makes the device send a frame cyclically (a heartbeat) until the buffer is written again or cleared
with `None`.

## Tests

`tests/io_can` runs two mock nodes end to end through the real host event loop: a full node with every optional
item (internal loopback through the filters, TX events with markers, bus-off and recovery, buffers, terminator, test
pins) and a minimal node with all of them left out.
