# ww_calloop

`wire_weaver_server` on a [calloop](https://crates.io/crates/calloop) event loop, for programs that are not on
tokio (a Wayland compositor, a GUI on its own loop).

- `LoopClock`: the server's timers on the loop's.
- `TcpIncoming`: TCP clients for a byte-stream listener, with WebSocket (`WsFraming`) or preamble framing.
- `bind_udp`: a `UdpListener` whose socket the loop watches.

`Server::serve` is a future for the loop's executor (`calloop::futures::executor`); connections, limits, shutdown
and peer identity are `wire_weaver_server`'s. See the "Host server" page of the docs.
