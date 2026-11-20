# ww_hub

> The ww hub: one process per user that holds WireWeaver devices and serves every program (SRV-13)

One process per user that holds WireWeaver devices and serves every program of the user over a local socket.
This crate is the hub as a library, so any program can host it: `ww hub serve`, or an app that calls
`main_if_hub` first in `main` so the client library can start the hub from the app's own executable (CLT-39,
`wire_weaver_client::hub`). One hub per user (an OS lock on `hub.lock`, released on a crash), the socket in the
runtime dir, a `Preamble` on each connection picking the hub's own API (`ww_hub_api`) or a device, device
tunnels opened on demand and closed after the last one leaves, and clock sync for every open device serving
`ww_timebase::Clock`. Depends on `ww_hub_api`, `ww_local_socket`, `ww_link` and `wire_weaver_client`.

See `design/ww-hub.md` and `docs/transport/hub.md` in the [main repo](https://github.com/vhrdtech/wire_weaver)
for the full design.
