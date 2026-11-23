# wire_weaver_server changelog

## Unreleased

### 🚀 Features

- The authorization hook sees items and effects below a mount slot, from the backend's learned API (SRV-79).
- `Server::trust_forwarders`: take a trusted forwarder's word on which client a request is from (SRV-79).
- `mount::Mounts`, `Forwarder`, `Connect`, `LocalSocket`: mount slots forward to backend processes, a link per front connection (SRV-78).
- `Forwarder`: a backend that goes ends the streams and observed levels under its slot with `Close` (SRV-78).
- `Server::mounts` reroutes global paths into mounted APIs before authorization; `Backend::cancel` gets cancels of forwarded requests (SRV-78).
- `mount::MountedTraits`: global trait paths into mounted APIs, from each backend's learned introspection; own tree first (SRV-77).
- The authorization hook sees a mount slot's `presence` as a read and its `api` as the last step (SRV-76).
- A hook may answer `Verdict::Pending`: the request waits for the answer within its deadline and cancel, other shared requests go on (SRV-72).
- The hook sees the steps of a keyed array's companion (`index_of`, `key`, `keys`), which had no items before.

### 🐛 Fixes

- A mount slot's API is learned before its presence turns `Up`, so the hook never misses it (SRV-79).
- Takes `wire_weaver_client` without its default features: `in_process` no longer pulls in USB, so Android builds.

### 📚 Documentation

- `Resource::items` carries the items' effects (API-58).

### ⚠️ Breaking

- A path below a slot whose API isn't learned has no items; it was the slot's `api` as last step (SRV-79).
- `mount::Forwarder::forward` takes the handler's `cx.call()`: the backend is told which client called (SRV-79).
- `Authorize`, `Denied` and `Resource` are `wire_weaver::authorize`'s, re-exported here (SRV-72): `check` returns a `Verdict` (`Result<(), Denied>` converts with `.into()`), a closure becomes a hook through `from_fn(..)`, and `Resource::items` is a slice of `Step` (name, id, index, effects, meta) instead of `(&ApiItemOwned, Option<u32>)`; `resource.api` with `item_chain(path)` still gives the items' model.

- `PumpId` names an element of a stream array too, `PumpId::indexed(id, &[index])` (CLT-58): `PumpId(n)` becomes `PumpId::new(n)`, and it is no longer `Copy`.
- The shutdown future of `serve` and `accepting` completes with a `ShutdownReason` (SRV-67): clients hear it; `pending()` becomes `pending::<()>()`.
- `MakeBackend` has `type Pumps` and `Serves` has `type Pumps`: implementors add `type Pumps = Pumps;`; `Connection` is `Connection<P = Pumps>` (SRV-73).
- `local_socket::LocalSocketListener` is now `LocalSocketIncoming`, a source for `byte_stream::listen` with `PreambleFraming`; `HANDSHAKES_MAX` moved to `byte_stream` (TR-19).
- `Listener::accept` returns `Option<Accepted>`: `None` says no more connections come, and `serve` ends once its connections ended (TR-19).

### 🐛 Fixes

- `byte_stream::TcpIncoming` drops a client whose `set_nodelay` fails instead of ending `serve` (SRV-75).

### 🚀 Features

- `udp::UdpListener::stop_handle` and `UdpStop`: stop one UDP listener while its connections go on (SRV-75).
- `Backend::end`: a step after the connection ended, before the backend is dropped, within `shutdown_grace` (SRV-74).
- `opened::opened`, `OpenedSender::serve` and `OpenedListener`: serve connections the program opened itself (SRV-67).
- `ShutdownReason`: a shutdown future of `DisconnectReason` tells clients that reason instead of `Shutdown` (SRV-67).
- Stream items ready at shutdown go out before the disconnect (SRV-67).
- `local_socket::peer_of`: the `Peer` of a client a program accepted on a local socket itself (SRV-67).
- `Pumps::add_ending` / `LocalPumps::add_ending`: a pump that sends one more event, the stream's `Close`, when its items end.
- `LocalPumps`, `Local` and `LocalConnection`: connections whose stream pumps take streams that are not `Send`, for single-threaded services (SRV-73).
- `Serves`: the service and pumps a `serve_trait` server forwards to (SRV-49).
- `byte_stream::is_transient` and `ACCEPT_RETRY_DELAY` are public, for sources on other event loops; `UdpListener::local_addr` works with any `StdUdpSocket` readiness (SRV-70).
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
