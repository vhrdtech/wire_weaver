# wire_weaver_server changelog

## Unreleased

### 🚀 Features

- New crate (SRV-12): `Server::serve` serves many connections of a `Listener`, each with its own backend, with limits and a graceful shutdown, on any async runtime.
- `Backend` (SRV-12): what a connection sends between requests, with `wait`, `next_wake` and `poll`; every backend implements it, empty by default.
- `Fanout<T>` and `Subscription<T>` (SRV-12): one sender, a queue per subscriber; a slow subscriber loses its oldest items and is told.
