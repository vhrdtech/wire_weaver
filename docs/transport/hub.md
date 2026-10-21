---
level: advanced
---

# Local sockets, the ww hub and who holds a device

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

## Who holds a device (CLT-38)

Every program linking `wire_weaver_client` that opens a USB interface or an RTT probe writes a lock file first,
whether or not a hub is involved — this is always on, not behind a feature flag. The registry lives under the
user's runtime directory:

- Linux: `$XDG_RUNTIME_DIR/ww/holders/`
- macOS: `$TMPDIR/ww/holders/`
- Windows: `%LOCALAPPDATA%\ww\run\holders\`

The directory is created `0700` the first time it's needed, or checked to already be owned by the current user —
a shared or world-writable runtime directory is refused rather than trusted.

### Device keys

Each device gets one lock file, named after a stable key:

- USB: `usb:<vid>:<pid>:<serial>[:<iface>]` (lower-case hex `vid`/`pid`, the WireWeaver interface number once
  `connect()` has found it)
- RTT: `rtt:<probe serial>:<chip>`

`wire_weaver_client::holders::key_for` builds one from a `DeviceRef`; the file name on disk escapes everything
but ASCII letters, digits, `.`, `-` and `_` (a USB serial number is a string the device chose, so it's treated as
untrusted input, not trusted to be a safe path component).

### Claiming

Before claiming the USB interface or attaching to the probe, the holder takes an exclusive
[`std::fs::File::try_lock`](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock) (`flock` on Unix,
`LockFileEx` on Windows — stable since Rust 1.89, no extra crate) on `<runtime dir>/ww/holders/<key>.lock`, and
keeps the file open for as long as it holds the device:

```rust,ignore
use wire_weaver_client::holders::{self, DeviceRef, Mode};

let key = holders::key_for(DeviceRef::Usb { vid, pid, serial, iface: Some(iface_number) });
match holders::claim(&key, Mode::Direct) {
    Ok(claim) => { /* keep `claim` alive for as long as the device is open */ }
    Err(holders::ClaimError::HeldBy(holder)) => { /* another WireWeaver program has it: `{holder}` says who */ }
    Err(holders::ClaimError::Io(e)) => { /* couldn't even write the registry; not fatal, just unrecorded */ }
}
```

The OS releases the lock the moment the holding process exits, cleanly or not (a crash, `kill -9`), so a stale
claim never lingers. The file's content can go stale when that happens — the content is just a few `key = value`
lines written before the lock is taken, nothing keeps it in sync with the lock itself — but that's harmless: both
`holder_of` and `list` only ever read the content *after* confirming the lock is actually held by someone right
now, never from content alone. Mode is `direct` for every holder today; once the hub exists, it claims devices as
`mode = hub`.

Content lines, extensible by design (a reader ignores any key it doesn't know, so a later addition — such as
richer client metadata — never breaks an older reader):

```text
key = usb:046d:c52b:ABCD1234:0
pid = 4821
program = dg
exe = /home/user/.cargo/bin/dg
mode = direct
since = 1771234567
ww_version = 0.5.0
```

### Busy errors name the holder

A connection refused because another WireWeaver program already holds the device says who, from that program's
own lock file content (`Holder`'s `Display`):

```text
device is already open by another WireWeaver program or connection; held by dg, pid 4711, direct, since 14:02:03 UTC
```

A program that doesn't speak WireWeaver (probe-rs's own CLI, `minicom`, `dfu-util`) never writes a lock file, so
there's nothing to read. On Linux, the busy error falls back to scanning `/proc/*/fd` for whichever process has
the device's node (`/dev/bus/usb/BBB/DDD`) open, and names it the same way; on other platforms it just says
"another program". `wire_weaver_client::holders::holder_of_usb_device(vid, pid, serial)` exposes that scan
directly, for RTT probes attached as a plain USB device underneath.

### Reading the registry

`holders::holder_of(key)` answers for one device, `holders::list()` for every device currently held — both
report only devices someone actually holds right now (the OS lock state, never stale content on its own). This is
the API the future hub's `Registry::devices()` and `ww hub who` build on; direct-mode programs can use it too, for
example to warn before trying to claim a device that's already busy.
