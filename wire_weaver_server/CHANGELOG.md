# wire_weaver_server changelog

## Unreleased

### 🚀 Features

- `Backend` requires `WireWeaverSharedApiBackend` (SRV-12): every request goes through an entry that takes the origin.
- Shared calls and reads (SRV-65 `receiver_model`) of a connection run side by side, up to `Limits::calls_max`; the rest runs alone behind them (SRV-12).
- `Connection::origin()` (SRV-12): handlers read the peer and the connection id from `cx.call()`.
- `Pumps`, `PumpId` and `Connection::pumps` (SRV-12): stream items sent as they come, also while calls run; `Subscription` is a `Stream`.
- `Limits::requests_max` (SRV-12): requests that run at once across all connections; one over it is answered `Overloaded` and never runs.
- New crate (SRV-12): `Server::serve` serves many connections of a `Listener`, each with its own backend, with limits and a graceful shutdown, on any async runtime.
- `Backend` (SRV-12): what a connection sends between requests, with `wait`, `next_wake` and `poll`; every backend implements it, empty by default.
- `Fanout<T>` and `Subscription<T>` (SRV-12): one sender, a queue per subscriber; a slow subscriber loses its oldest items and is told.
- `Server::accepting` and `Accepting::next` (SRV-12): each connection as a future the program spawns, for multi-threaded runtimes.
- A server that shuts down disconnects its clients with `DisconnectReason::Shutdown` (SRV-12).
