# wire_weaver_server

The host side counterpart of `wire_weaver_client`: one API served to many clients at once.

- A listener hands out connections with the peer the transport saw: in-process (feature `in_process`) or a local
  socket, also one systemd socket activation passed (feature `local_socket`).
- Each connection goes through link setup, gets its own backend from a factory and is served until its link goes
  down.
- Limits on connections and on what one connection may queue, and a graceful shutdown.
- No async runtime of its own: `Server::serve` is a future for the program's executor, timers come from a
  `ww_device::Clock`.

See the "Host server" page of the docs.
