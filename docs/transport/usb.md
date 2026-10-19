---
level: basic
---

# USB

USB is the primary transport for embedded devices: fast, always available on a dev board, and — because
WireWeaver uses a vendor-specific interface with plain bulk or interrupt endpoints — it needs **no drivers** on
Windows, macOS or Linux. The host side is [nusb](https://github.com/kevinmehall/nusb), a pure-Rust USB library, so
there is no libusb to ship either.

## Host side

USB is the default feature of `wire_weaver_client`. A generated client connects to the first device that matches its
default filter, or to the one picked with a config closure:

```rust
let blinky = Blinky::new().connect().await?;                       // default config of the driver crate
let blinky = Blinky::config(|c| c.usb_vid_pid(0xC0DE, 0xCAFE).serial_eq(serial)).connect().await?;
```

Devices can be selected by VID:PID, by physical port (`usb_port_chain`, survives re-enumeration), by serial string,
user label or API (`user_label_eq`, `implements_api`), or just "any WireWeaver device" (`.usb()`). All of these are
read from string descriptors, so the host doesn't have to open a device to know what it is.

`ww list` shows every connected device with its API (install the CLI with
`cargo install --git https://github.com/vhrdtech/wire_weaver wire_weaver_cli`):

```
$ ww list
LOCATION             PRODUCT               SERIAL                    API               HASH              LABEL
usb 003-1 c0de:cafe  Nucleo G0B1RE blinky  21002200175036344B333720  blinky_api@0.1.0  042c28cc0c9da99b  Nucleo on the desk
```

Filters can be combined: `--api blinky_api@^0.1` (name with an optional SemVer requirement), `--label <label>`,
`--product <substring>`, `--serial <substring>`, `--usb-path 003-1`, ... `--all` also lists devices without an API
id, and `--plain` prints one line per device for scripts. See [Command line tool](../cli.md) for all options and
for keeping the selection in a project `ww.toml`. From code, `wire_weaver_client::list_usb_devices()` gives the same
list.

## Device side

`wire_weaver_usb_embassy` (in the `mcu/` workspace) puts the generated server on top of any
[embassy-usb](https://docs.rs/embassy-usb) driver. `usb_init()` returns the USB device to run and the server, each
runs in its own task:

```rust
const MAX_USB_PACKET_LEN: usize = 64; // 64 for Full Speed, 512 (Bulk) or 1024 (Interrupt) for High Speed
const MAX_MESSAGE_LEN: usize = 1024;  // longest WireWeaver message
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
let link_config = LinkConfig::new(
    blinky_api::BLINKY_API_FULL_GID,
    server_impl::api_hash(),
    ww_client_server::COMPACT_VERSION,
);
let (usb, server) = usb_init(
    driver,
    USB_BUFFERS.init(UsbBuffers::default()),
    UsbTimings::fs_higher_speed(),
    link_config,
    server_impl::API_ID,
    |config| {
        config.product = Some("Nucleo G0B1RE blinky");
        config.serial_number = Some(embassy_stm32::uid::uid_hex());
    },
);
spawner.spawn(unwrap!(usb_task(usb)));            // usb.run().await
spawner.spawn(unwrap!(ww_server_task(server, state))); // server.run(&mut state).await
```

`examples_mcu/nucleo_g0b1re/src/bin/usb_blinky.rs` is the complete firmware, `examples/blinky` the host side.
`server.run()` is the whole event loop; to wait on other things next to it (UART data, timers), use
`server.wait()` / `server.handle()` in your own loop, see [link layer](ww_link.md#how-the-device-runs-it).

A driver with endpoint kinds `usb_init()` doesn't name (e.g. embassy-stm32's double-buffered bulk endpoints on USB
v4 parts) can use `usb_init_with_endpoints()` instead: same as `usb_init()`, with a closure allocating the OUT and
IN endpoints.

## What you get

- **Plug and play**: hot-plug is handled — a device that disappears fails outstanding requests and can be
  re-connected without touching the rest of the application.
- **High message rate**: many small requests and events are packed into one USB packet, so throughput is bounded
  by bandwidth, not by packets per second. Large messages are transparently split across packets and reassembled.
- **Full duplex**: sending and receiving never block each other on the host, so a device that is busy answering
  cannot stall the host and vice versa (see [ww_link](ww_link.md#why-two-halves) for why this matters).
- **Version and identity check** during link setup, before any application data flows.

## Supported MCUs

Any MCU with an `embassy-usb` driver. Tested on STM32G0B1 (Full Speed), STM32H743 (Full Speed), STM32H725 with a
ULPI PHY (High Speed, 480 Mbit/s) and RP2350, see the [firmware examples](../examples/examples.md).

## Limitations and notes

- Endpoint addresses (`0x01`/`0x81`) and interface `0` are currently fixed.
- Isochronous endpoints are not supported (nor needed).
- On Linux no udev rule is required for enumeration, but opening the device needs read/write access to
  `/dev/bus/usb/...` — a rule granting that to your user is the usual setup.

More: [how USB works inside](usb_internals.md) — device layout, identity strings, framing, transfer queues,
reconnection.

## See also

- [How USB works inside](usb_internals.md)
- [ww_link](ww_link.md) — session lifecycle, messages, the two-task design
- [Framer](ww_framer.md) — packing and splitting messages
- [Transport overview](overview.md)
