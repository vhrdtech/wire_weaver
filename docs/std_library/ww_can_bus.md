---
level: basic
---

# ww_can_bus

Part of the [IO trait family](io_family.md). `ww_can_bus` is the WireWeaver API of CAN controllers: a USB-CAN
adapter, a gateway, a node with a CAN port, a bus analyzer or debugger. The host configures the bus, sends and
receives classic, CAN FD and CAN XL frames, and watches the controllers' error state. It is `no_std` by default and
depends on `wire_weaver` and `ww_si` (`Baud` bit rates, `Hertz` clocks, `Ohm` terminators, `Second` timestamps).

```toml
[dependencies]
ww_can_bus = "0.1"                                    # classic and FD
ww_can_bus = { version = "0.1", features = ["xl"] }   # plus CAN XL
```

## Traits

| Trait | What it is | Who implements it |
|---|---|---|
| `Can` | One controller with its transceiver: configure, receive, send, error state; optional items for everything only some controllers have | Every device |
| `CanGroup` | Controllers that share clock, power and message RAM (all FDCAN instances of an STM32): `interface[]` of `Can`, enable / disable, clock source and frequency, group-wide performance | Devices that mirror such hardware |
| `CanTimeTriggered` | Time-triggered CAN (ISO 11898-4) of one controller | Controllers with TTCAN (FDCAN1 of an STM32H7) |
| `CanXl` | CAN XL frames of one controller (`xl` feature) | Controllers with CAN XL |

A simple bridge puts its controllers straight into its API and implements the six required items of `Can`
(`capabilities`, `configure`, `config`, `status`, `rx` and `tx`):

```rust
#[ww_api_root]
trait Bridge {
    ww_impl!(can[]: ww_can_bus::Can);
}
```

Every item beyond the six required ones is optional; a simple node leaves them all out:

```rust
ww_codegen!(
    my_api::Bridge for Server,
    server = true,
    omit = [
        "can.set_mode", "can.recover", "can.tx_abort", "can.tx_events", "can.bus_errors",
        "can.protocol_status", "can.bus_load", "can.detect_timing", "can.filters", "can.set_layout",
        "can.rx_buffers", "can.tx_buffers", "can.transceiver", "can.terminator", "can.test_pins"
    ],
);
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
serialized frames; a frame the controller can't send as configured (FD with FD off, anything in a mode that doesn't
send) is dropped and reported as `TxOutcome::Rejected` on `tx_events`.

## Configuration

```rust
pub struct CanConfig {
    pub mode: Mode,                       // Off, Config, Normal, Restricted, ListenOnly, Internal/ExternalLoopback, Test, Sleep
    pub nominal: Timing,                  // arbitration phase
    pub fd: Option<FdConfig>,             // data phase timing, TDC, ISO / non-ISO; None: classic only
    pub options: Options,                 // one_shot, presume_ack, triple_sample, tx_pause, tx_order, rx_overflow,
                                          // edge_filtering, protocol_exception, wake_on_bus
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
`Common(InvalidArgument)`. `configure` takes the controller off the bus and back as its hardware needs (frames
queued for sending stay queued) and ends in `CanConfig::mode`.

```rust
let actual = node
    .can()
    .configure(CanConfig {
        mode: Mode::Normal,
        nominal: Timing::Rate { rate: Baud::kilo(500u16), sample_point: 875 },
        fd: Some(FdConfig {
            data: Timing::Rate { rate: Baud::mega(2u8), sample_point: 750 },
            tdc: Tdc::Auto,
            iso: true,
        }),
        options,
        bus_off_recovery: Some(Second::milli(100u8)),
    })
    .call()
    .await?;
let nominal = actual.nominal.segments().unwrap(); // e.g. prescaler 1, seg1 139, seg2 20 at 80 MHz
```

## Frames

```rust
pub struct Frame<'i> {
    pub id: CanId,                                  // Standard(U11) or Extended(U29)
    pub kind: FrameKind,                            // Data, Remote { len }, Fd { brs, esi }, DataLongDlc { dlc }
    pub data: MaxLen<RefVec<'i, u8>, MAX_DATA_LEN>, // up to 64 bytes
}
```

`CanId::standard(0x123)` and `CanId::extended(0x18FF_0001)` build an ID (`None` when out of range).
`FrameKind::DataLongDlc` is a classic frame with 8 bytes and a DLC of 9-15 on the wire, for bus debuggers.

A received frame is an `RxFrame { frame, filter }`, `filter` being the index of the filter element that accepted it
(`None` without filters or for non-matching frames). A frame to send is a `TxFrame { frame, marker }`: the marker
comes back in the frame's `TxEvent`, so a host matches events to what it sent (M_CAN's message marker, MCP2518FD's
sequence number, gs_usb's echo ID). On the wire a classic standard frame with 8 bytes takes 12 bytes as an
`RxFrame` or `TxFrame`, a 64-byte FD frame with a marker 69.

A device with CAN XL carries those frames on a trait of its own, see [CAN XL](ww_can_bus_reference.md#can-xl).

## Time

Received frames, TX events and bus errors carry their time in the event head: a `Second` of the device's one clock
(`ww_timebase::Clock`), in the prefix and number type the device picks (usually a `u32` of µs, which wraps every
71.6 minutes; the host unwraps it around `Clock::now()`). `Capabilities::timestamps` says how fine it is and where in
the frame it was taken: start of frame (M_CAN, bxCAN in TTCM, MCP2518FD), end of frame (gs_usb), or by software when
the frame was read out (no hardware timestamps); `None`, and heads without a time, on devices without a clock.

## Status and errors

`status` is an observable read-only [property](../api/properties.md): `Status { state, tec, rec, last_error,
last_data_error }`, `state` being `Stopped`, `Active`, `Warning`, `Passive`, `BusOff` or `Sleeping`. The device
sends it to observers when the state changes:

```rust
let mut status = node.can().observe_status().await?;
while let Ok(s) = status.recv().await {
    if s.state == ErrorState::BusOff { node.can().recover().call().await?; }
}
```

Methods return `CanError`: [`CommonError`](../api/errors.md#common-errors) as `Common` (unsupported, invalid
argument, in use, wrong state, ...), plus `OutOfFilters { max }` (fall back to fewer or wider filters),
`OutOfMemory { available }` (a smaller layout or schedule) and `BusOff` (recover first). Each method lists what it
returns with `#[errors(..)]`. Errors on the bus are not errors of a call: they are counted in `Status` and reported
one by one on `bus_errors`.

## Tests

`tests/io_can` runs two mock nodes end to end through the real host event loop: a full node with every optional
item (internal loopback through the filters, TX events with markers and queue times, bus-off and recovery, explicit
modes and configuration steps, raw protocol status, timing detection, bus load, sleep and wake-up, transceiver,
buffers, terminator, test pins) and a minimal node with all of them left out. `tests/io_can_group` runs a group of
two controllers (clock selection, enable / disable, independent interfaces), time-triggered operation and CAN XL
frames.

## More

The [ww_can_bus reference](ww_can_bus_reference.md) has every item of `Can` and which controllers have what,
controller groups (`CanGroup`), CAN XL, the debugger items, what the device measures, sleep and the transceiver,
filters and message memory, and time-triggered CAN.
