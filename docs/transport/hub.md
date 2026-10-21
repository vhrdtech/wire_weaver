---
level: advanced
---

# Local sockets and the ww hub

A USB interface, a debug probe or a serial port can be open in one process only. Local sockets let other programs on
the same PC reach a device through the process that holds it: a simulator or host-emulated firmware serving its API
to several tools, and, being built on this, the ww hub, one small process per user that holds the devices and serves
every program (SRV-13, see the [feature tracker](../features.md)).

## Host side

Enable the `local_socket` feature of `wire_weaver_client` and select the socket by path:

```rust
let sim = Blinky::config(|c| c.ipc_path("/run/user/1000/ww/sim.sock".into()))
    .connect()
    .await?;
```

Everything above the medium is the same as with [USB](usb.md) or [WebSocket](websocket.md): [link setup](ww_link.md),
version check, introspection, pings, timeouts, streams.

## Device side

`wire_weaver_client::local_socket::device::device_medium()` gives the `MessageTx` / `MessageRx` pair for a
`ww_device::Server` on a listener, one client at a time (the next one waits until the current one leaves), with
`TokioClock` from `in_process` as the clock:

```rust
let path = ww_local_socket::runtime_dir()?.join("sim.sock");
let listener = ww_local_socket::listen(&path, true)?;
let (tx, rx) = device_medium(listener, "sim 0.1.0", 1024);
let mut server = ww_device::Server::new(link_config, tx, rx, TokioClock::new(), &mut scratch, &mut event_scratch);
```

`replace = true` removes a socket file left behind by an earlier run; pass `false` when another live process might
own the name.

## Who can connect

Sockets are for the user that runs the programs, nobody else:

- `ww_local_socket::runtime_dir()` is the place for them: `$XDG_RUNTIME_DIR/ww` on Linux (a per-user tmpfs;
  `/tmp/ww-<uid>` without it), `$TMPDIR/ww` on macOS (per user), `%LOCALAPPDATA%\ww\run` on Windows. It is created
  with mode 0700; a directory that belongs to another user, or a symlink, is refused, a looser mode of our own is
  tightened. `WW_RUNTIME_DIR` puts it elsewhere (tests).
- On Linux the socket file is 0600. macOS can't set the mode of a socket file, the 0700 directory guards it.
- On Windows the listener is a named pipe whose access list allows only its owner and refuses network logons (the
  default would let everyone read).
- Both ends ask the OS who is on the other side (`SO_PEERCRED` / `getpeereid`, the pipe's process and its token's
  user on Windows) and drop a connection of another user. That covers a name squatted by someone else, as named
  pipes live in one namespace for all users.

## Wire format

A connection starts with a preamble from the client and an answer from the server, then carries
[ww_link](ww_link.md) messages; a reliable stream, so [ww_framer](ww_framer.md) isn't used.

```
client → server   | "WWHB" | version: u8 = 1 | target: u8 | key_len: u16 LE | key: UTF-8 |
server → client   | "WWHB" | version: u8 = 1 | status: u8 | hub_version_len: u8 | hub_version: UTF-8 |
then, both ways   | len: u32 LE | kind: u8 | payload |    len counts kind + payload
```

- `target` 0 is the hub's own API (no key), 1 a device by its key (`usb:c0de:cafe:<serial>`); a process serving a
  single device accepts any key, the client sends an empty one.
- `status`: 0 ok, 1 no such device, 2 the device is held by another program, 3 preamble version not served, 4 the
  client's WireWeaver versions are not served. The server closes the connection after any status but 0, and the
  client's connect error says which, with the server's version.
- The preamble is frozen: a later layout gets a new version number, and a reader of any version can still read an
  answer up to the server's version string, so two programs built years apart can at least tell each other what
  they are.
- Messages are held back until the event loop flushes, so the messages of one turn go out in one write. A message
  longer than the receiver's limit drops the connection.
