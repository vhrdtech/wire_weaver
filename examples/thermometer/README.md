# thermometer

Client of the thermometer example device (`thermometer_api`), and the device simulated on a PC over WebSocket.

```sh
cargo run -p thermometer --example thermometer_sim    # terminal 1: the simulated device
cargo run -p thermometer --example thermometer_read   # terminal 2: one sample, a stream, an observed gain
```

`thermometer_sim --udp 9000` serves UDP instead; `thermometer_connect` reads a sample over `usb`, `websocket` or `udp`, and `thermometer_keep_trying` goes on through a device that is stopped and started again.

`tests/it` runs the same simulated device in process, on tokio's paused clock.
