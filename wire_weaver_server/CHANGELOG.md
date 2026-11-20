# wire_weaver_server changelog

## Unreleased

### 🚀 Features

- New crate (SRV-12): `Server::serve` serves many connections of a `Listener`, each with its own backend, with limits and a graceful shutdown, on any async runtime.
