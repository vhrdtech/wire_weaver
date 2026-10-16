---
level: advanced
---

# ww_can_bus reference

Everything of [`ww_can_bus`](ww_can_bus.md) beyond the common case: every item of `Can`, controller groups, CAN XL,
the items for bus debuggers, filters and message memory, and time-triggered CAN.

## `Can`

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

    #[optional] fn set_mode(mode: Mode) -> Result<Mode, CanError<'i>>;
    #[optional] fn recover() -> Result<(), CanError<'i>>;
    #[optional] fn tx_abort() -> u32;
    #[optional] #[counted] stream!(tx_events: TxEvent);
    #[optional] #[counted] stream!(bus_errors: BusError);
    #[optional] fn protocol_status() -> ProtocolStatus;
    #[optional] property!(ro bus_load: BusLoad);
    #[optional] fn detect_timing(candidates: MaxLen<RefVec<'i, Timing>, 16>, per_candidate: Second)
        -> Result<Option<CanConfig>, CanError<'i>>;
    #[optional(filters)] fn set_filters(filters: FilterConfig<'i>) -> Result<(), CanError<'i>>;
    #[optional] fn set_layout(layout: MemoryLayout) -> Result<MemoryLayout, CanError<'i>>;
    #[optional(rx_buffers)] fn rx_buffer(index: u16) -> Result<Option<BufferedFrame<'i>>, CanError<'i>>;
    #[optional(tx_buffers)] fn tx_buffer(index: u16, frame: Option<TxFrame<'i>>, period: Option<Second>)
        -> Result<(), CanError<'i>>;
    #[optional(transceiver)] fn set_transceiver(mode: TransceiverMode) -> Result<TransceiverMode, CanError<'i>>;
    #[optional(transceiver)] property!(ro transceiver_state: TransceiverState);
    #[optional(terminator)] fn set_termination(termination: Termination) -> Result<Termination, CanError<'i>>;
    #[optional(terminator)] property!(ro termination: Termination);
    #[optional] fn test_pins(tx: TxPin) -> Result<BusLevel, CanError<'i>>;
}
```

| Item | Required | What it is for |
|---|---|---|
| `capabilities()` | ✓ | Controller clock, bit timing limits, FD and TDC, modes and options, FIFO / buffer / filter counts, memory, timestamps, transceiver modes, terminator values |
| `configure(CanConfig)` | ✓ | Mode, nominal and data bit timing, TDC, options and bus-off recovery in one call, answered with what was set |
| `config()` | ✓ | The configuration in effect, for a host attaching to a running node |
| `status` | ✓ | Error state, TEC / REC and the last protocol errors; observable, so a bus-off or a wake-up reaches the host without polling |
| `rx` | ✓ | Received frames, a counted stream (unit: frame) with the receive time in the head |
| `tx` | ✓ | Frames to send, a credit sink: the host never overruns the device's TX queue |
| `set_mode` | | Switch the mode alone, keeping timing, filters and layout (see [Debuggers](#debuggers)) |
| `recover()` | | Start the bus-off recovery on request |
| `tx_abort()` | | Drop every frame queued and not on the bus yet |
| `tx_events` | | When each sent frame went on the bus, with its marker and how long it waited, or why it didn't |
| `bus_errors` | | Every error detected on the bus: kind, phase, direction |
| `protocol_status()` | | The controller's raw protocol state |
| `bus_load` | | Bus load the device measures, sent once per window |
| `detect_timing` | | Find the bit timing of a running bus |
| `set_filters` (`filters`) | | Acceptance filters: exact ID, mask or range, into a FIFO (optionally high priority), a dedicated buffer, or rejected |
| `set_layout` | | Divide the message memory between filters, FIFOs, buffers and TX events |
| `rx_buffer` (`rx_buffers`) | | The latest frame of a dedicated RX buffer, without streaming every frame |
| `tx_buffer` (`tx_buffers`) | | A dedicated TX buffer: send once, or cyclically every `period` by the device |
| `set_transceiver`, `transceiver_state` (`transceiver`) | | Transceiver standby / silent / sleep, bus wiring faults |
| `set_termination`, `termination` (`terminator`) | | A switchable terminator (relay, MOSFET) |
| `test_pins` | | Production test: drive TX, read RX |

Which controllers have what:

| | bxCAN | M_CAN (STM32 FDCAN) | MCP2518FD | ESP32 TWAI | SocketCAN | gs_usb / candleLight |
|---|---|---|---|---|---|---|
| CAN FD | – | ✓ | ✓ | – | ✓ | ✓ |
| `tx_events` | – | TX event FIFO | TEF | – | own-message echo | echo frames |
| `bus_errors` | LEC | PEA / PED, LEC | diagnostics | error capture | error frames | BERR |
| `protocol_status` | ESR | PSR, ECR | TREC, BDIAG | status, error capture | state, berr-counter | GET_STATE |
| `set_filters` | filter banks | std / ext elements | 32 filters | one mask | socket filters | – |
| `set_layout` | – | message RAM (full M_CAN) | FIFO RAM | – | – | – |
| `rx_buffer` | – | dedicated RX buffers (full M_CAN) | FIFO of depth 1 | – | – | – |
| `tx_buffer` | mailboxes | dedicated TX buffers | FIFOs | – | BCM | – |
| `recover` | without ABOM | INIT | – | `initiate_recovery` | restart | reset |
| `Mode::Sleep` | sleep | clock stop | sleep | – | – | – |
| transceiver, terminator | board | board | board | board | `termination` | TERMINATION |
| `CanTimeTriggered` | TTCM (timestamps only) | TTCAN (FDCAN1 of an H7) | – | – | – | – |

## `CanGroup`

```rust
#[ww_trait]
pub trait CanGroup {
    ww_impl!(interface[]: Can);
    fn group_capabilities() -> GroupCapabilities<'i>;
    fn enable() -> Result<GroupClock, CanError<'i>>;
    fn disable() -> Result<(), CanError<'i>>;
    property!(ro group_state: GroupState);
    #[optional(clock)] fn set_clock(clock: GroupClock) -> Result<GroupClock, CanError<'i>>;
    #[optional] fn performance() -> Performance;
}
```

On STM32 every FDCAN instance hangs off one kernel clock and one message RAM: none can be clocked, powered down or
re-laid out alone. `CanGroup` is that hardware. The group starts disabled (clock off, its lowest power state);
`enable()` clocks it with the source and frequency set last (or the device's default) and answers the clock in
effect, then each `interface[i]` is configured on its own. `disable()` needs every controller in `Mode::Off`.

Advanced nodes pick the clock to hit an exact bit timing: `set_clock(GroupClock { source, frequency })` while
disabled, from `GroupCapabilities::clock_sources` (name and frequency range of each), answered with the nearest
frequency the dividers reach. The controllers then report that clock in `Capabilities::clock` and count their time
quanta in it. A bridge leaves `set_clock` out and runs on the default. A standalone controller with its own
oscillator choice (MCP2518FD) is a group of one.

`performance()` is what the group guarantees together (frames and bytes per second passed on without loss): a
device whose controllers share a CPU or a link to the host knows it, the host can't.

A device that mirrors its hardware puts a group into its API instead of single controllers, with the time-triggered
and XL parts next to it as arrays indexed like the controllers (valid only at indices that have the hardware):


```rust
#[ww_api_root]
trait Analyzer {
    ww_impl!(can: ww_can_bus::CanGroup);                 // can.interface[i]
    ww_impl!(can_tt[]: ww_can_bus::CanTimeTriggered);    // can_tt[i] is controller i
    ww_impl!(can_xl[]: ww_can_bus::CanXl);               // can_xl[i] is controller i
}
```

## CAN XL

CAN XL frames carry up to 2048 bytes, and a credit sink sizes the device's buffer for its largest item. So they are
not part of `Frame` but of `CanXl`, behind the `xl` cargo feature: devices without XL neither reserve 2 KiB
buffers nor carry the types.

```rust
#[ww_trait]
pub trait CanXl {
    fn configure_xl(config: Option<XlConfig>) -> Result<Option<XlConfig>, CanError<'i>>;
    #[counted] stream!(rx_xl: XlFrame<'i>);
    #[credit] sink!(tx_xl: XlTxFrame<'i>);
}
```

`XlFrame` has the 11-bit priority ID, VCID, SDU type, SEC, acceptance field and 1-2048 data bytes. `XlConfig` sets
the XL data phase timing, TDC, PWM symbols for transceiver mode switching (CAN SIC XL) and data phase error
signalling; it needs CAN FD configured on the controller. A device sending full-size XL frames takes WireWeaver
messages of at least 2.1 KiB.

## Debuggers

`configure` sets a node up in one call. A bus debugger keeps every step in hand:

- `set_mode` switches the mode alone, keeping bit timing, filters and layout: `Mode::Config` to stay off the bus
  (TX recessive) for configuration-only steps, then `Restricted`, `ListenOnly`, a loopback, `Test`, `Sleep` or back
  to `Normal`. Items that need configuration mode (`set_layout`, `set_filters` on most controllers,
  `CanTimeTriggered::configure_tt`) switch through it and back by themselves, and stay in it when called there.
- `protocol_status()` reads the controller's raw protocol state: activity (synchronizing, idle, receiving,
  transmitting), the counters, its error logging counter, last error codes of both phases, flags of the last
  received FD frame, protocol exceptions, the measured transceiver delay and message memory ECC errors. Reading
  resets what the controller resets on a read.
- `bus_errors` reports every error with kind, phase and direction; `test_pins` drives TX and reads RX.
- Raw register access is not part of this crate: a device that offers it implements
  [`ww_memory`](ww_memory.md) next to it.

```rust
let can = dev.can().interface(0);
can.set_mode(Mode::Config).call().await?;
can.set_layout(MemoryLayout { standard_filters: 64, ..layout }).call().await?;
can.set_filters(filters).call().await?;
can.set_mode(Mode::ListenOnly).call().await?;
let ps = can.protocol_status().call().await?;
```

## Measured on the device

What the device sees better than the host is measured there:

- receive, TX event and bus error times (stream heads, see [Time](ww_can_bus.md#time));
- `TxEvent::queued`: how long a frame waited from the device accepting it to its start of frame (queue,
  arbitration, retransmissions);
- `bus_load`: busy time of the bus per window, frames and error frames, sent to observers once per window;
- `ProtocolStatus::tdc_value`: the transceiver loop delay the controller measured;
- `detect_timing(candidates, per_candidate)`: listens without acknowledging at each candidate timing and answers
  the first one a frame came in at without errors, leaving the controller configured with it in `ListenOnly`.

## Sleep and the transceiver

`Mode::Sleep` puts the controller to sleep; with `Options::wake_on_bus` bus activity wakes it, back in the mode it
was in, and `status` goes from `Sleeping` to `Active`. The transceiver is switched separately (`set_transceiver`:
`Standby`, `Silent`, `Sleep`); its `transceiver_state` property reports wiring faults (CAN_H / CAN_L open or
shorted, dominant timeout, over temperature) as they appear.

## Filters, memory and dedicated buffers

```rust
node.can().set_filters(FilterConfigOwned {
    filters: MaxLen::new(vec![
        Filter { matching: FilterMatch::Exact(CanId::standard(0x100).unwrap()), action: FilterAction::Buffer(2) },
        Filter {
            matching: FilterMatch::Range { from: CanId::standard(0x200).unwrap(), to: CanId::standard(0x20F).unwrap() },
            action: FilterAction::Fifo { fifo: 0, high_priority: false },
        },
    ])?,
    non_matching_standard: NonMatching::Reject,
    non_matching_extended: NonMatching::Accept { fifo: 1 },
    reject_remote: false,
}).call().await?;
let latest = node.can().rx_buffer(2).call().await?; // the last 0x100 frame, if one came
```

The first matching filter wins. A device maps the list to its hardware (filter banks, elements, masks) and answers
`OutOfFilters` or `Unsupported` (a range on a mask-only controller) when it can't. On controllers with a
configurable message memory (`Capabilities::memory`, in 32-bit words), `set_layout` sets how many filters, FIFO and
buffer elements and TX events it holds and how many data bytes an element takes; controllers of a group share the
memory, so a layout that doesn't fit next to the others is `OutOfMemory`. `tx_buffer(index, Some(frame),
Some(period))` makes the device send a frame cyclically (a heartbeat) until the buffer is written again or cleared
with `None`.

## Time-triggered CAN

```rust
#[ww_trait]
pub trait CanTimeTriggered {
    fn configure_tt(config: Option<TtConfig>) -> Result<Option<TtConfig>, CanError<'i>>;
    fn set_schedule(triggers: MaxLen<RefVec<'i, Trigger>, MAX_TRIGGERS>) -> Result<(), CanError<'i>>;
    property!(ro tt_status: TtStatus);
}
```

`TtConfig` sets the ISO 11898-4 level, the role (slave, or potential time master with a priority), the reference
message ID, the matrix cycle length and the network time unit; the controller must be in `Mode::Config` or
`Mode::Off`. The schedule is a sorted list of triggers (time mark, cycle repeat and offset, and what to do: send
the reference message, send a TX buffer exclusively or in a merged arbitrating window, check a reception, watch,
end of list), bounded by the trigger memory (`OutOfMemory`). `tt_status` reports synchronization, master role, the
basic cycle and the error level.
