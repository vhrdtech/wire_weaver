# wire_weaver_server changelog

## Unreleased

### ⚠️ Breaking

- `local_socket::LocalSocketListener` is now `LocalSocketIncoming`, a source for `byte_stream::listen` with `PreambleFraming`; `HANDSHAKES_MAX` moved to `byte_stream` (TR-19).
- `Listener::accept` returns `Option<Accepted>`: `None` says no more connections come, and `serve` ends once its connections ended (TR-19).

### 🚀 Features

- `Server::authorize` asks an `Authorize` hook before every handler, with the caller and the `Resource`; denied requests get `Denied` (SRV-46).

- Deadline and cancel (SRV-47): a request is dropped when its context's deadline runs out, queued or running, and answered `DeadlineExceeded`; a client's `Cancel` drops it and answers `Cancelled`. A request that runs alone no longer waits without a bound for slow shared ones before it. Handlers read the caller's trace and what is left of its deadline from `cx.call()`. The server says it reads contexts in link setup; its own error answers reach a client of an older model as `UserStr`.
- `udp::UdpListener` (feature `udp`): a connection per host of a datagram socket, by its address, up to `PEERS` at once; `UdpListener::bind` on tokio (feature `tokio`), `PeerAddr` (SRV-69).
- `Accept<Tx, Rx, E>`: the result of `Listener::accept`, named (TR-19).
- `byte_stream::WsFraming` (TR-19): WebSocket clients on any byte-stream source; the URL path names the service.
- Byte-stream sources: `TcpIncoming` and `stdio` (feature `tokio`), `handed_in` with `StreamSender::serve_stream(stream, peer)` for streams the program accepted (TR-19).
- `byte_stream` (TR-19): `listen` on any source of streams with its `Framing`; `StreamListener::service` serves several APIs on one source.
- `local_socket::LocalSocketListener` (SRV-12, feature `local_socket`): clients on a local socket, also one systemd passed, with their process as the peer.
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
