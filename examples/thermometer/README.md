# thermometer

Client of the thermometer example device (`thermometer_api`), and the device simulated on a PC over WebSocket.

```sh
cargo run -p thermometer --example thermometer_sim       # terminal 1: the simulated device
cargo run -p thermometer --example thermometer_read      # terminal 2: one sample, the mode and the gain
cargo run -p thermometer --example thermometer_observe   # the gain, observed while it changes
cargo run -p thermometer --example thermometer_stream    # continuous and triggered samples, commands
cargo run -p thermometer --example thermometer_channels  # the inputs as an array of channels
cargo run -p thermometer --example thermometer_units     # numbers with units, no device needed
```

`thermometer_sim --udp 9000` serves UDP instead; `thermometer_connect` reads a sample over `usb`, `websocket` or `udp`, and `thermometer_keep_trying` goes on through a device that is stopped and started again.

`tests/it` runs the same simulated device in process, on tokio's paused clock, and checks what the examples print against the book's copies in `docs/generated/thermometer/`.
