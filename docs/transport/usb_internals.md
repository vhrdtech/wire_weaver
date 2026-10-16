---
level: advanced
---

# How USB works inside

What the [USB](usb.md) transport puts in the descriptors, how it frames messages and how the host keeps the bus
busy. Read it to write a device without `wire_weaver_usb_embassy`, to tune throughput, or to debug enumeration.

```
 application        wire_weaver_client                  device
 ───────────        ────────────────────────────        ──────────────────
 Request bytes ──► TxCore ──► ww_link msg ──► framer ──► OUT endpoint ──► embassy-usb
 Event bytes   ◄── RxCore ◄── ww_link msg ◄── framer ◄── IN endpoint  ◄── ...
```

The USB transport plugs into the generic event loop through a small message-level interface: it receives
`(kind, bytes)` pairs from the tx side and hands decoded `(kind, bytes)` pairs to the rx side. Everything USB
specific lives in the implementation of that interface.

## Device layout

Interface `0` with a pair of bulk (or interrupt) endpoints, `0x01` OUT and `0x81` IN; a vendor-specific class
keeps the OS from attaching its own driver. Max packet size is read from the endpoint descriptor — 64 B on full
speed, 512 B (Bulk) / 1024 B (Interrupt) on high speed — and used as the [framer](ww_framer.md) frame size. Nothing
else is assumed about the descriptor.

TODO: Interrupt vs Bulk

## Device identity strings

Everything needed to tell devices apart is in string descriptors, which the OS reads during enumeration and
exposes to anyone listing devices, so the host **does not need to open a device** to know what it is (on Linux,
listing doesn't even need the udev rule):

| Descriptor                        | Contents                                                 |
| --------------------------------- | -------------------------------------------------------- |
| Manufacturer (iManufacturer)      | Vendor name                                              |
| Product (iProduct)                | Device description, e.g. `Nucleo G0B1RE blinky`          |
| Serial (iSerialNumber)            | Unique serial number, e.g. MCU UID                       |
| WireWeaver interface (iInterface) | API id: `ww:<crate>@<version> h=<hash>[ l=<user label>]` |

For example: `ww:blinky_api@0.1.0 h=042c28cc0c9da99b l=Nucleo on the desk`. `h=` is the first 8 bytes of the API
hash without docs, the same one reported during link setup. `l=` is optional and always last, so the label may
contain anything, including spaces. Unknown `key=value` fields are ignored by the parser, so more can be added
later. See `wire_weaver::api_id` for the encoder and parser.

The API id is an interface string rather than the product or serial string so that the product stays
human-readable and the serial stays stable across firmware updates (Windows keys device instances by serial).
Interface strings are available without opening the device on Linux (sysfs), macOS (IOKit) and Windows (for
composite devices, which `usb_init()` always configures). The host falls back to the product string if no
interface string is an API id.

On the device, server codegen emits the `API_ID` constant, built entirely at compile time, and it is passed to
`usb_init()`:

```rust
let (usb, server) = usb_init(driver, buffers, timings, link_config, server_impl::API_ID, |config| {
    config.product = Some("Nucleo G0B1RE blinky");
    config.serial_number = Some(embassy_stm32::uid::uid_hex());
});
```

A user label is only known at runtime (e.g., loaded from flash), `with_label` appends it into a buffer
without any formatting, truncating it to the 126 UTF-16 characters a USB string descriptor can hold:

```rust
static API_ID_BUF: StaticCell<[u8; 126]> = StaticCell::new();
let api_id = wire_weaver::api_id::with_label(server_impl::API_ID, label, API_ID_BUF.init([0; 126]));
```

A changed label becomes visible after re-enumeration. The control buffer must hold the longest string
descriptor (`2 + 2 * 126` bytes), `UsbBuffers` has 256 bytes for it.

On the host, `DeviceInfo` (used for filtering and in `AmbiguousDeviceChoice` errors) is filled from these strings,
so `.user_label_eq(..)` and `.implements_api(..)` filters work without opening devices.
`wire_weaver_client::list_usb_devices()` lists all devices reporting an API id, the same as
[`ww list`](usb.md#host-side).

## Framing

Messages are packed into packets with `ww_framer` using the USB configuration from `ww_link`:
`U2Head` (1–2 byte heads), a CRC-16 per split message, no tail. Packet boundaries are significant: the framer
relies on USB delivering each transfer as sent, and a short packet marks the end of a frame. The CRC is not there
for the wire (USB already checks packets) but to reject stale or reordered packets after a reconnect.

Small requests are accumulated for the window the device asks for in `DeviceInfo` (typically a few hundred µs)
and flushed as one packet; control messages (link setup, ping, disconnect) are flushed immediately.

## nusb transfer queues

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

## Reconnection

On any exit path the host sends `Disconnect` when it can, waits a few milliseconds for the transfer to leave, and
releases the interface. Outstanding requests complete with `Disconnected`; stream subscriptions can be kept
(`DisconnectKeepStreams`) and resume on the next connection, which may be to a different device or transport.
