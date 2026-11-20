# ww_device

> WireWeaver device side: sans-IO link state machine and event loop building blocks, no_std and no alloc

Mirrors the host side (`wire_weaver_client`) in layers, while leaving the event loop in user code: `link::DeviceLink`
is a sans-IO link state machine (link setup and version check, frame accumulation window, pings, peer timeout),
fed messages and time and handing back messages to send; `transport` has async `MessageTx`/`MessageRx` traits,
with `FramedTx`/`FramedRx` over packets (USB, CAN, ...) and `StreamTx`/`StreamRx` over bytes (RTT, UART, ...).
Media are features: `rtt`, `ws` (WebSocket, also over `embassy-net` sockets), `udp` (also over `embassy-net`).

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
