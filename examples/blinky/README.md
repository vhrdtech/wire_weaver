# blinky

> Client of the blinky example device, and the device simulated on a PC over WebSocket

The host side of `blinky_api`: `Blinky` implements `WwClient` and carries the generated calls `led_on()`,
`led_off()`, `set_led(on)` and `blinks()`. `simulated()` runs the device itself (`blinky_device`) over WebSocket.

```sh
cargo run -p blinky --example blinky_sim      # terminal 1: the simulated device at ws://127.0.0.1:8080/ww
cargo run -p blinky --example blinky_host     # terminal 2: calls it; `-- --usb` calls a board instead
```

`blinky_simple`, `blinky_blocking`, `blinky_rtt` (feature `rtt`), `blinky_ws`, `blinky_ws_device` and `blinky_udp`
(feature `udp`) reach a board over USB, RTT, WebSocket and UDP. The book's chapters 1.5 to 1.9 are built on this
example: https://ww.vhrd.tech/
