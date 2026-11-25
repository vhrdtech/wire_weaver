# thermometer

Client of the thermometer example device (`thermometer_api`), and the device simulated on a PC over WebSocket.

```sh
cargo run -p thermometer --example thermometer_sim    # terminal 1: the simulated device
cargo run -p thermometer --example thermometer_read   # terminal 2: one sample, a stream, an observed gain
```

`tests/it` runs the same simulated device in process, on tokio's paused clock.
