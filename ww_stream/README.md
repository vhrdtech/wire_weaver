# ww_stream

> Counted and reliable WireWeaver streams: positions, loss accounting, acknowledgements, stores, backfill

Counted and reliable WireWeaver streams (API-30): positions in units, exact loss accounting, acknowledgements
and go-back-N resends, stores that keep data while the host is slow or gone, and backfill after a reconnect with
the latest values first. Everything here is sans-IO: generated server and client code (or a test) feeds the
state machines what arrived and sends what they return. `tx::Counted`/`tx::Reliable` are the sender state
machines (device side), `rx::Receiver` the host-side receiver, `store::StreamStore`/`store::RamRing` the stores,
and `observe::ObserveTable` tracks observed properties with an earliest-deadline-first budget (SRV-18). The wire
types live in `ww_client_server::stream`.

See `docs/api/stream_reliability.md` in the [main repo](https://github.com/vhrdtech/wire_weaver) for the design
and reasons.
