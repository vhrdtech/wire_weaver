# USB

USB is the primary transport for embedded devices: fast, always available on a dev board, and — because
WireWeaver uses a vendor-specific interface with plain bulk or interrupt endpoints — it needs **no drivers** on
Windows, macOS or Linux. The host side is [nusb](https://github.com/kevinmehall/nusb), a pure-Rust USB library, so
there is no libusb to ship either.

## What you get

- **Plug and play**: connect by VID:PID, by physical port (bus + port chain, survives re-enumeration), by serial
  string, or just "any WireWeaver device". Hot-plug is handled — a device that disappears fails outstanding
  requests and can be re-connected without touching the rest of the application.
- **High message rate**: many small requests and events are packed into one USB packet, so throughput is bounded
  by bandwidth, not by packets per second. Large messages are transparently split across packets and reassembled.
- **Full duplex**: sending and receiving never block each other on the host, so a device that is busy answering
  cannot stall the host and vice versa (see [ww_link](ww_link.md#why-two-halves) for why this matters).
- **Version and identity check** during link setup, before any application data flows.
- **Tracing**: with the `usb-tracing` feature every packet in both directions is published over iceoryx2 for
  external inspection.

```rust
let client = ClientConfig::new()
    .usb_vid_pid(0xC0DE, 0xCAFE)      // or .usb_port_chain(..), .serial_eq(..), .usb()
    .connect().await?;
```

## Supported MCUs

## How it works

```
 application        wire_weaver_client                  device
 ───────────        ────────────────────────────        ──────────────────
 Request bytes ──► TxCore ──► ww_link msg ──► framer ──► OUT endpoint ──► embassy-usb
 Event bytes   ◄── RxCore ◄── ww_link msg ◄── framer ◄── IN endpoint  ◄── ...
```

The USB transport plugs into the generic event loop through a small message-level interface: it receives
`(kind, bytes)` pairs from the tx side and hands decoded `(kind, bytes)` pairs to the rx side. Everything USB
specific lives in the implementation of that interface.

### Device layout

Interface `0` with a pair of bulk (or interrupt) endpoints, `0x01` OUT and `0x81` IN; a vendor-specific class
keeps the OS from attaching its own driver. Max packet size is read from the endpoint descriptor — 64 B on full
speed, 512 B (Bulk) / 1024 B (Interrupt) on high speed — and used as the [framer](ww_framer.md) frame size. Nothing else is assumed about the
descriptor.

TODO: Interrupt vs Bulk

### Framing

Messages are packed into packets with `ww_framer` using the USB configuration from `ww_link`:
`U2Head` (1–2 byte heads), a CRC-16 per message, no tail. Packet boundaries are significant: the framer relies on
USB delivering each transfer as sent, and a short packet marks the end of a frame. The CRC is not there for the
wire (USB already checks packets) but to reject stale or reordered packets after a reconnect.

Small requests are accumulated for the window the device asks for in `DeviceInfo` (typically a few hundred µs)
and flushed as one packet; control messages (link setup, ping, disconnect) are flushed immediately.

### nusb transfer queues

USB transfers are fire-and-forget: the host submits a buffer and gets a completion later. To keep the bus busy,
several transfers must be **in flight at once**, especially for IN where the host has to be listening before the
device can push anything. The transport keeps two independent queues, each driven by a small task that submits
buffers and forwards completions over a channel:

| Direction | In flight | Purpose                                                                                          |
| --------- | --------- | ------------------------------------------------------------------------------------------------ |
| IN (rx)   | 64        | Pre-submitted read buffers; the device can burst up to 64 packets before the host must catch up. |
| OUT (tx)  | 4         | A small pool of write buffers; a write waits for a completion when all four are on the wire.     |

Buffers are allocated once through nusb (so they can be DMA-friendly where the platform supports it) and recycled:
a completed IN buffer is resubmitted right after its payload is staged into the framer; a completed OUT buffer goes
back into the pool. There are no timeouts on individual transfers — a device that stops responding is detected by
the link-level peer timeout (10 s without any message), which keeps a device that is legitimately busy for a
moment (flash erase, say) from being cut off.

### Reconnection

On any exit path the host sends `Disconnect` when it can, waits a few milliseconds for the transfer to leave, and
releases the interface. Outstanding requests complete with `Disconnected`; stream subscriptions can be kept
(`DisconnectKeepStreams`) and resume on the next connection, which may be to a different device or transport.

## Limitations and notes

- Endpoint addresses (`0x01`/`0x81`) and interface `0` are currently fixed.
- Isochronous endpoints are not supported (nor needed).
- On Linux no udev rule is required for enumeration, but opening the device needs read/write access to
  `/dev/bus/usb/...` — a rule granting that to your user is the usual setup.

## See also

- [ww_link](ww_link.md) — session lifecycle, messages, the two-task design
- [ww_framer](ww_framer.md) — head layout, splitting and reassembly
- [Transport overview](overview.md)
