# ww_calloop changelog

## Unreleased

### 🚀 Features

- New crate (SRV-70): `LoopClock`, `TcpIncoming` and `bind_udp` serve `wire_weaver_server` on a calloop event loop.

### 🐛 Fixes

- `TcpIncoming` drops a client whose `set_nodelay` or loop registration fails instead of ending `serve` (SRV-75).
