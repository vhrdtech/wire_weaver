# blinky

> Client of the blinky example device: the project shape a host program copies

Depends on `blinky_api` (the two-method API trait) and `wire_weaver_client`. `Blinky` implements `WwClient` and
picks up the generated `Commander`-based client of `blinky_api`'s trait, so a user program only has to open a
connection and call `on()` / `off()`.

Run against a real device over USB (`cargo run --example blinky_simple`), against a simulated one in the same
process (`blinky_blocking`), or over RTT, WebSocket and UDP with their own feature (`rtt`, `ws`, `udp`) and example.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
