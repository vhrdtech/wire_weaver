# RTT

RTT (SEGGER Real-Time Transfer) turns the debug probe that is already attached to a dev board into a transport:
an _up_ channel (device → host) and a _down_ channel (host → device) are ring buffers in the device's RAM, read
and written by the probe over SWD / JTAG. No USB peripheral, no UART wiring, and it works next to `defmt`
logging, which uses another RTT channel.

Useful when a board has no USB, when the USB peripheral is taken by something else, or to try an API out before
any transport hardware exists.

## Device side

Device support is in `ww_device` behind the `rtt` feature, on top of the
[rtt-target](https://docs.rs/rtt-target) crate. Create the channels with `rtt_init!`, hand one pair to
`rtt_server()` and run the same [Server](ww_link.md#how-the-device-runs-it) loop as for USB:

```rust
use ww_device::rtt::{RttBuffers, RttConfig, rtt_server};
use ww_device::{EmbassyClock, LinkConfig};

const MAX_MESSAGE_LEN: usize = 1024;
static RTT_BUFFERS: StaticCell<RttBuffers<MAX_MESSAGE_LEN>> = StaticCell::new();

let channels = rtt_init! {
    up: {
        0: { size: 1024, mode: NoBlockSkip, name: "defmt" }
        1: { size: 2048, mode: NoBlockSkip, name: "ww_up" }
    }
    down: {
        0: { size: 2048, mode: NoBlockSkip, name: "ww_down" }
    }
};
rtt_target::set_defmt_channel(channels.up.0);

let link_config = LinkConfig::new(
    blinky_api::BLINKY_API_FULL_GID,
    server_impl::api_hash(),
    ww_client_server::COMPACT_VERSION,
);
let mut server = rtt_server(
    link_config,
    channels.up.1,
    channels.down.0,
    EmbassyClock,
    RttConfig::default(),
    RTT_BUFFERS.init(RttBuffers::new()),
);
server.run(&mut state).await;
```

`examples_mcu/rp2/src/bin/ww_rtt.rs` is a complete example (`just run rp2 ww_rtt`), serving the same blinky API as
`usb_blinky`.

### Sizing

- `MAX_MESSAGE_LEN` is the longest message the device accepts and the longest reply it can serialize, reported to
  the host as is. `RttBuffers` takes `3 * MAX_MESSAGE_LEN` plus a few bytes of framing overhead.
- RTT channel buffers are separate, allocated by `rtt_init!`. A chunk (one or more messages, up to
  `MAX_MESSAGE_LEN` plus framing) is written to the up channel in one go, and in `NoBlockSkip` mode a write that
  does not fit is skipped entirely, so the **up channel must be at least `MAX_MESSAGE_LEN + 5` bytes**, better
  twice that. `NoBlockTrim` works too. `BlockIfFull` must not be used: it spins forever when no host reads.
- The down channel only needs to hold what the host writes between two polls.

### Timing

RTT has no notion of a connection and cannot wake the device: the down channel is polled every
`RttConfig::poll_interval` (1 ms by default), which bounds the request latency, and the same interval is used
to retry a write into a full up channel. A host that is gone shows up as an up channel that stays full for
`RttConfig::write_timeout` (500 ms), and in any case as the link's peer timeout (10 s without a message).

A probe polls the target on its own schedule, so the round trip is set by the host side (probe-rs polling
interval, SWD clock) rather than by the device.

## Framing

RTT is a byte stream: what the device writes as one chunk may be read by the host in any number of pieces, and
the other way around, so the [framer](ww_framer.md) is used in _stream mode_ — every message is a `Full` one, never
split across chunks (`Tx::write_full`), and the receiver cuts messages out of the stream by their length
(`StreamTx` / `StreamRx` in `ww_device`). The configuration is `RttHead` / `RttChecksum` / `RttTail` from
`ww_link`: `U2Head`, no checksum and no tail, since the ring buffers are in RAM and cannot corrupt bytes, and
`NoBlockSkip` writes are atomic, so nothing is lost mid-message either.

Without a delimiter there is nothing to re-synchronize on: a message larger than the receiver's buffer is skipped,
but the bytes that follow are misinterpreted until the transport is reset. The device reports its
`MAX_MESSAGE_LEN` during link setup, a host must never send more than that.

Many small messages are still packed into one chunk, accumulated for the window the device asks for in
`DeviceInfo` (1 ms by default); control messages are flushed right away.

## Host side

TODO: `wire_weaver_client` with the `rtt` feature (probe-rs), `ClientConfig::rtt(..)`.

## See also

- [ww_link](ww_link.md) — session lifecycle, messages, how the device runs it
- [ww_framer](ww_framer.md) — head layout, `write_full` for stream media
- [USB](usb.md) — the primary transport
