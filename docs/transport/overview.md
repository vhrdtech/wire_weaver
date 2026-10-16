---
level: basic
---

# Transport protocols

Generated server and client code does no IO: the same API runs over any medium, and the medium is picked when
connecting. On the host, each transport is a feature of `wire_weaver_client`; on the device, of `ww_device` (or the
`wire_weaver_usb_embassy` crate for USB).

## Which one to pick

| Transport                   | Use it for                                                                          | Host feature | Device side                          |
|-----------------------------|-------------------------------------------------------------------------------------|--------------|--------------------------------------|
| [USB](usb.md)               | MCU boards: fast, no drivers on any OS, hot-plug, many devices told apart by serial | `usb` (default) | `wire_weaver_usb_embassy` (embassy-usb) |
| [RTT](rtt.md)               | a board with a debug probe attached and no USB to spare, early bring-up              | `rtt`        | `ww_device`, `rtt` feature            |
| [WebSocket](websocket.md)   | devices on a network that can run TCP, Linux boards, simulators in another process  | `ws`         | `ww_device`, `ws` feature             |
| [UDP](udp.md)               | devices on a network, telemetry where a lost event beats a stall                    | `udp`        | `ww_device`, `udp` feature            |
| in-process                  | tests and simulators: the device runs in the same process                           | `in_process` | `ww_device::Server` on channels       |

CAN bus, I2C and UART as transports are planned (the `can`, `i2c` and `uart` features of `wire_weaver_client` are
placeholders without code yet).

## Connecting

The medium is chosen in the client config, the rest of the code doesn't change:

```rust
let blinky = Blinky::new().connect().await?;                                    // USB, driver's default filter
let blinky = Blinky::config(|c| c.usb().serial_eq(serial)).connect().await?;   // USB, one device of several
let blinky = Blinky::config(|c| c.rtt("RP2040".into(), None)).connect().await?; // RTT through a debug probe
let blinky = Blinky::config(|c| c.websocket_url("ws://192.168.1.10:8080/ww")).connect().await?;
let blinky = Blinky::config(|c| c.udp_addr("192.168.1.10:9000")).connect().await?;
let blinky = Blinky::config(|c| c.in_process_path("sim/blinky".into())).connect().await?;
```

`connect_blocking()` does the same without an async runtime of your own. After connecting, every transport behaves
the same: link setup with a version check, introspection, pings and timeouts, streams.

## How the layers fit

Messages from the generated code are packed into frames by the [framer](ww_framer.md): many small messages share
one USB packet or UDP datagram, a big one is split across several. The [link layer](ww_link.md) on top of it sets up
the session (versions, buffer sizes), keeps it alive with pings and ends it with a reason; it also has the device's
event loop. WebSocket and in-process connections carry whole messages and skip the framer (the simulated USB device
of `in_process::usb_device` keeps it, to test exactly what goes over USB).
