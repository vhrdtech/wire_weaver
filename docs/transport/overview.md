---
level: advanced
---

# Connecting in depth

[Choosing a transport](../basic/transports.md) picks a transport and runs the thermometer over two of them. This page
lists the host features and device crates of every transport, and the steps of a connect that can be timed.

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

## Where a connect's time goes

A connect runs in steps, each a debug `tracing` span named in `wire_weaver_client::connect_spans`, so any subscriber
can time them:

| Span                    | Step                                                                               |
|-------------------------|------------------------------------------------------------------------------------|
| `ww_connect`            | the whole `connect()` or `connect_blocking()`                                      |
| `ww_connect_validate`   | the config is checked, the client's own introspection data prepared                |
| `ww_connect_select`     | a transport finds the device and starts the event loop (USB: the device list)      |
| `ww_connect_link`       | until the link is up and the device's API version known                            |
| `ww_connect_open`       | within the link step: the medium is opened (USB: open and claim the interface)     |
| `ww_connect_learn`      | the device's API: introspection data unless the client has it, the API match       |
| `ww_connect_introspect` | within the learn step: introspection data from the cache or the device             |

`just connect-bench` prints these per round, for in-process devices or a blinky board on USB (`--transport usb`),
with the generated async or blocking client or `DynClient` (`--client`), one client or many at once (`--clients`).
It runs a new process with an empty cache, then one with the machine's own: the first round of each is a cold
connect, later rounds are warm.

## How the layers fit

Messages from the generated code are packed into frames by the [framer](ww_framer.md): many small messages share
one USB packet or UDP datagram, a big one is split across several. The [link layer](ww_link.md) on top of it sets up
the session (versions, buffer sizes), keeps it alive with pings and ends it with a reason; it also has the device's
event loop. WebSocket and in-process connections carry whole messages and skip the framer (the simulated USB device
of `in_process::usb_device` keeps it, to test exactly what goes over USB).
