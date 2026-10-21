---
level: advanced
---

# Local sockets, the ww hub, who holds and who connects

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

## The hub process (SRV-28)

The hub is crate `ww_hub`, with its API in `ww_hub_api`. `ww hub serve` runs it in the foreground; a program can also
host it: `ww_hub::main_if_hub()` first in `main` makes the executable serve as the hub when started with
`WW_HUB_SERVE` set (sccache-style), so a client can start the hub from the app itself (see
[below](#reaching-devices-through-the-hub-clt-39)).

- One hub per user: it holds an OS lock on `hub.lock` in the runtime dir for its whole life (released by the OS on a
  crash) and writes its pid, version and start time into it. A second one exits at once (`Exit::AlreadyRunning`).
  A `hub.sock` without the lock held is left over from a hub that is gone and is replaced.
- Every connection starts with the preamble: target `Hub` is the hub's own API, a device target gets a tunnel to
  that device (see [Device tunnels](#device-tunnels-srv-29)).
- It exits after `idle_exit` (5 min, `WW_HUB_IDLE_EXIT` in seconds, `ww hub serve --idle-exit`) without connections;
  `Hub::keep(seconds)` holds it up for a bench, `Hub::shutdown()` ends it.

### Its API

The hub serves `ww_hub_api` like any device serves its API (a generated `no_alloc = false` server per connection),
so introspection and the dynamic client work on it. `ww_hub::client::HubClient` is the typed client:

```rust
let hub = HubClient::connect().await?; // ClientConfig::hub_api(): the user's hub socket
let info = hub.info().call().await?; // version, pid, exe, start time, socket
let devices = hub.registry().devices().call().await?;
let clients = hub.registry().clients().call().await?;
let mut events = hub.registry().events().call().await?;
```

- `info()` and `handover(socket)` are frozen: a client of any version can ask what hub runs (`handover`, SRV-30, is
  not served yet).
- `registry().devices()`: the WireWeaver USB devices plugged in (enumerated, not opened: the hub opens a device only
  when a program asks for it) and every device a program holds, from the lock files above, as `HeldElsewhere` with
  the holder; the devices the hub holds itself as `Open`, with their tunnels. `take(key)` opens a device and keeps it
  open without clients (`Opened` / `Closed` events).
- `registry().clients()`: the programs connected to the hub's API, with what their link setup said about them
  (program, kind, user, host, instance id) and the pid the OS reports.
- `registry().events()`: `Plugged`, `Unplugged`, `HolderChanged` (from hotplug events and a rescan every 2 s),
  `ClientJoined`, `ClientLeft`. A client that falls behind misses events and reads the lists again.

## Reaching devices through the hub (CLT-39)

A program holds its device directly, as it always did, unless the hub pays off: a long-lived GUI, service or notebook
gains nothing from a hub in between, a loop of short `ww` calls in a terminal does (each call would enumerate, open,
set up the link and fetch the API again). `connect()` decides per device, after it picked one with the usual
filters; nothing changes in user code. It needs `wire_weaver_client`'s feature `hub` (on in `ww`).

### The modes

| `HubMode` | `WW_HUB` | What `connect()` does |
|---|---|---|
| `Auto` (default) | `auto` | Through the hub when the hub already holds the device, or when the device was opened often lately (below); else directly, holding the device exclusively. |
| `Prefer` | `prefer` | Through the hub, starting one if none runs; directly when none can start or it can't serve the device. |
| `Required` | `required` | Through the hub, starting one if none runs, or fail. |
| `Direct` | `off` | Never through the hub, even past the open counter; it says so in the log when the hub holds the device (the open then fails as busy, naming the hub). |

```rust
let device = MyDevice::config(|c| c.hub(HubMode::Prefer)).connect().await?;
```

`ClientConfig::hub` wins; a config that doesn't set it takes `WW_HUB`, else `Auto`. So `WW_HUB=off` turns the hub off
for every program that leaves the choice to its user, and a program that must never share its device (a test bench,
the hub itself) sets `Direct` in code.

"The hub holds the device" is what the hub's registry says (`Registry::devices()`: the device open by the hub, or
held in its name, `mode = hub` in the lock file). Without a running hub (nobody holds `hub.lock`) nothing is asked.

### Repeated opens

Every open in `Auto` mode leaves a line in `<runtime dir>/ww/opens/<key>` (`<unix ms> <weight>`, older lines dropped,
rewritten under an OS lock). When the weights within the window reach the limit, the open goes through the hub,
started if none runs, and the hub keeps the device open for its linger: the rest of the loop costs a socket round
trip per call. The default is 3 within 60 s (`ClientConfig::hub_opens(OpenCounter { opens, window })`,
`WW_HUB_OPENS=3/60`). A client of kind `Cli` (the default when the parent process is a shell, see
[client meta](#who-connects-client-meta-clt-41)) counts 1, every other kind 1/4: a loop in a terminal goes through
the hub from its third call, a GUI restarted twice doesn't.

### Starting the hub

`wire_weaver_client::hub::ensure_running(start)` is what `connect()` uses, and any program can call it:

1. Someone holds `hub.lock`: a hub runs, wait until it answers on its socket.
2. Else take `hub.start.lock` (waiting up to 5 s for another starter), check `hub.lock` again: two programs starting
   a hub at once get one, the second finds the first one's.
3. Remove a socket file left behind by a hub that is gone (nobody holds `hub.lock`).
4. Start the newest hub executable, detached (its own process group on Unix, `DETACHED_PROCESS |
   CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows, stdin and stdout closed, stderr into
   `<runtime dir>/ww/hub.log`), with `WW_HUB_SERVE=1`.
5. Poll the socket: connect and send the preamble for the hub's API every 20 ms, until an answer comes (2 s at most).
   This works the same on every OS; no pipe is inherited. A hub that exits before answering (and no other hub took
   `hub.lock`) fails it with its exit status.

In `Auto` and `Prefer` a hub that can't be had means a direct connection with a log line saying why; in `Required` it
is the connect error.

Which executable: `WW_HUB_EXE` when set, alone. Otherwise every one of these that answers `--hub-version` with a
version, the newest first (among equal versions, this order):

- the program itself, when it put `ww_hub::main_if_hub()` first in `main` (which registers it, and answers
  `--hub-version`), so an app shipped alone still gets a hub;
- `ww` next to the program's executable;
- `ww` on `PATH`.

None found: the error says how to get one (`cargo install wire_weaver_cli`, `WW_HUB_EXE`, or `main_if_hub()`).

### `ww hub take`

`ww hub take DEVICE` (a key such as `usb:c0de:cafe:0042`, or a serial number) starts the hub if needed and has it
open the device now and keep it, so every program reaches the device through the hub from its first call
(`wire_weaver_client::hub::take(key)`; `hub::key_of(&DeviceInfo)` gives a device's key).

### What is there today

The hub doesn't open devices yet (SRV-29): it answers a device tunnel with `UnknownDevice` and `take` with
`Unimplemented`. Until then `Auto` and `Prefer` fall back to a direct connection with a note and `Required` fails.
Only USB devices are routed, RTT and the other transports connect directly. `connect_blocking()` routes when it runs
on a multi-thread tokio runtime (Python's is one); on a current-thread runtime it connects directly.
### The `ww hub` command (TOOL-17)

`ww hub status | stop | who | keep | serve` ([reference](../cli_reference.md#ww-hub)) is a client of the API above,
except `who`, which also works without a hub. Behind `status` is `Hub::status()`: who started the hub (the
`WW_HUB_STARTED_BY` the starter set, else the parent process's name on Linux), the idle exit, the seconds left until it
exits or of a `keep`, and the open connections; `Registry::devices()` carries the location, label and API hash too
(the columns of `ww list`), which is what lets `ww list` answer from the hub.

`who` reads the three sources of [Who holds a device](#who-holds-a-device-clt-38): the lock files (the hub's own and
every direct-mode program's, `holders::list()`), then for a device without one the `/proc` scan of a program that
isn't ww (Linux; "another program" is all other platforms can say). It never opens a device, so it never disturbs the
holder. A hub that runs is not needed and not started for it.

`keep` calls `Hub::keep(seconds)`; `forever` is the largest value, `off` is `0`. Starting a hub by hand is
`ww hub serve`; programs start it by themselves ([Starting the hub](#starting-the-hub)), and `ww hub lend` waits for SRV-32.
## Device tunnels (SRV-29)

A program reaches a device through the hub with `ClientConfig::hub_device(key)`, the device's key as in
[Device keys](#device-keys):

```rust
let device = Blinky::config(|c| c.hub_device("usb:c0de:cafe:204B319B5431"))
    .connect()
    .await?;
```

Everything after the preamble is the same as talking to the device directly: link setup, version check,
introspection, calls, streams, properties, pings. Several programs can use one device at the same time this way; a
program that opens the device directly instead gets a [busy error](#busy-errors-name-the-holder)
naming the hub (mode `hub`).
`hub_device_at(socket, key)` names another hub socket (tests, a hub in another runtime dir).

### What the hub does

The hub runs one `wire_weaver_client` event loop per open device and gives each client its own link endpoint (a
`ww_device::DeviceLink`, as on a device). It never links a user API: it reads only a request's seq, path and kind.

- **Link setup.** The endpoint answers with the device's own `DeviceInfo` (its user API version and hash, API model
  version, accumulation window) and checks the client's `LinkSetup` against them, as the device would. A client is
  told it may send 4 bytes less than the device takes, room for a longer seq. The client's meta (CLT-41) shows in
  the hub's registry.
- **Seq remap.** Every request goes to the device with a seq of the hub's event loop; each answer gets the client's
  seq back and goes to that client only. Answers of plain calls are routed on the event loop's receive task straight
  into the client's queue.
- **Streams.** A stream path is opened on the device by its first client; later clients get the device's answer to
  that open, and its data goes to every client that opened it. The last close closes it on the device. A client
  that leaves closes its streams. Acknowledgements, resend requests and credit of a shared stream (counted or
  reliable streams) go to the device from the first client only.
- **Observed properties.** Each client's observe settings are kept per trait level and merged: the latest setting of
  a property wins, the budget is the largest asked for. The device gets the merged set, and updates go to every
  client of the level.
- **Introspection** is fetched from the device once and answered from the hub's copy afterwards.
- **Queues.** What goes to a client waits in its queue: stream data and property updates beyond 1 MiB are dropped
  and counted (a client that doesn't read must not hold up the device or the other clients), answers and stream
  sidebands are never dropped.
- **Open and close.** A device is opened when the first client asks for it and closed 30 s after the last one left
  (`WW_HUB_LINGER` in seconds, `HubConfig::linger`); `registry().take(key)` keeps it open without clients. The hub
  writes the device's lock file in mode `hub` (CLT-38), and the registry lists it as `Open` with its tunnels. A
  client killed mid-stream is a closed socket: its streams and observe settings are dropped as if it closed them.
- **Older and newer clients.** A request whose envelope the hub can't decode (a newer API model) goes to the device
  as it is, with only its seq changed; so does a client without meta.

### Limits

- Stream sinks and the sideband of a shared stream are the first client's.
- Streams opened on a trait path are not counted per client: each open and close goes to the device, and events of
  a path the hub didn't see opened go to every client.
- Loopback test messages are echoed to the client that sent the last ones (`ww usb-loopback` through the hub: one at
  a time).
- Device keys: `usb:`, `rtt:` (with the `rtt` feature, `ww` has it) and `inproc:<path>` (a device served in the
  hub's own process: simulators, tests).

### What it costs

Every message crosses a local socket twice more (client → hub, hub → client), with a task wake each way. On an idle
PC that is about 14 µs per round trip (in-process device: 20 → 34 µs); on a busy one each wake can also wait for a
CPU, so the cost grows with load. `tests/hub/examples/hub_loopback.rs` runs `ww usb-loopback`'s test directly and
through an in-process hub in alternating rounds and compares the medians:

```sh
cargo run --release -p hub --example hub_loopback -- usb:c0de:cafe:<serial> [seconds] [packet size|max] [rounds]
```

BENCH_TABLE

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
kind = Cli
user = roman
host = pc1
label =
purpose =
instance_id = 9318842301562216747
```

The last six lines are the holder's [client meta](#who-connects-client-meta-clt-41); a file written by an older
program lacks them and reads with those fields empty.

### Busy errors name the holder

A connection refused because another WireWeaver program already holds the device says who, from that program's
own lock file content (`Holder`'s `Display`):

```text
device is already open by another WireWeaver program or connection; held by dg, pid 4711, Cli, roman@pc1, direct,
since 14:02:03 UTC; to share a device between programs, let the ww hub hold it: `ww hub take`, or WW_HUB=prefer for
both programs (a hub comes with `ww`: cargo install wire_weaver_cli)
```

A program that doesn't speak WireWeaver (probe-rs's own CLI, `minicom`, `dfu-util`) never writes a lock file, so
there's nothing to read. On Linux, the busy error falls back to scanning `/proc/*/fd` for whichever process has
the device's node (`/dev/bus/usb/BBB/DDD`) open, and names it the same way; on other platforms it just says
"another program". `wire_weaver_client::holders::holder_of_usb_device(vid, pid, serial)` exposes that scan
directly, for RTT probes attached as a plain USB device underneath.

### Reading the registry

`holders::holder_of(key)` answers for one device, `holders::list()` for every device currently held — both
report only devices someone actually holds right now (the OS lock state, never stale content on its own). This is
the API the hub's `Registry::devices()` and [`ww hub who`](#the-ww-hub-command-tool-17) build on; direct-mode programs can use it too, for
example to warn before trying to claim a device that's already busy.

## Who connects: client meta (CLT-41)

A device, a log or a list of holders is more useful when it can say *who* is on the other end of a link: "`ww`, pid 4711, started by roman on pc1, a command line tool" instead of "a host". Every client therefore tells the device who it is in link setup, in an optional `client_meta` field of `ww_link`'s `LinkSetup`. The hub (SRV-13) and the holder lock files (CLT-38) show the same fields.

Nothing decides on it: it is free text for people, it is not authentication, and a client can say what it likes.

### What is sent

| Field | Meaning | Default |
|---|---|---|
| `program` | program name | file name of the running executable, without `.exe` |
| `app_version` | version of the program, free text | empty |
| `label` | a name the user gave this client | empty |
| `kind` | `Cli`, `Gui`, `Notebook`, `Service`, `Hub`, `Test` (`Unknown`; a kind added later reads as `Newer`) | `Cli` when the parent process is a shell, else `Unknown`; Python sets `Notebook` |
| `pid` | process id | this process |
| `user` | login name | `$USER`, `$LOGNAME` or `$USERNAME` |
| `host` | host name of the PC | the PC's host name |
| `purpose` | what the program does with the device, e.g. "DFU update" | none |
| `instance_id` | random number per client object, shared by all its connections | picked when the `ClientMeta` is created |

The parent is a shell when its process name is one of `bash`, `zsh`, `fish`, `sh`, `dash`, `ksh`, `nu`, `pwsh`, `powershell` or `cmd`: read from `/proc` on Linux, `proc_name` on macOS and a Toolhelp snapshot on Windows. A GUI, a service or a test says what it is itself.

Long values are cut (names to 48 bytes, version 24, purpose 64) so that the message stays small. A device with a tiny receive buffer gets a shortened form (program, user and host cut to 16 bytes, no label, version or purpose), and one that can't take even that gets none: the connection never fails because of the meta.

### Setting it

```rust
use wire_weaver_client::{ClientConfig, ClientKind, ClientMeta};

let config = ClientConfig::new()
    .usb()
    .client_meta(
        ClientMeta::new()
            .kind(ClientKind::Gui)
            .app_version(env!("CARGO_PKG_VERSION"))
            .label("bench 2")
            .purpose("calibration"),
    );
```

Everything not set is filled in. `no_client_meta()` sends nothing. The instance id is picked when `ClientMeta::new()` runs, so every connection made from clones of one config shares it, and two clients of the same program and pid are told apart. `wire_weaver_py`'s `connect()` sets the kind to `Notebook`.

### On the device

`ww_device::DeviceLink::peer_meta()` returns the last host's `PeerMeta` (program, user and host cut to 24 bytes, kind, pid, instance id) or `None` for a host that sent nothing. It is set before `LinkEvent::Up` is reported, so firmware that wants to log who connected reads it there:

```rust
if let Some(LinkEvent::Up) = server.handle(ready, &mut backend).await {
    if let Some(peer) = server.link().peer_meta() {
        info!("host: {} ({}) on {}", peer.program.as_str(), peer.pid, peer.host.as_str());
    }
}
```

It costs about 90 bytes of RAM in the `DeviceLink`.

### Compatibility

`client_meta` is appended to `LinkSetup` as `#[default = None] Option<ClientMeta>`, the same way the setup token was added. A device built before it reads the longer message and ignores the end; a host built before it sends a `LinkSetup` that ends before it and a new device reads `None`. Golden bytes of both forms are in the conformance corpus (`link_setup.without_meta`, `link_setup.with_meta`), and `ClientKind` has `future_version = Newer`, so a kind added later still decodes (as `Newer`). Adding a field to `LinkSetup` is wire-compatible, see [evolution rules](../evolution/rules.md).

## Servers on std: `no_alloc = false`

A server is generated for firmware by default: `no_alloc = true`, handlers take borrowed arguments and return
borrowed values (`&str`, `RefVec<'i, T>`, `Foo<'i>`), written straight into the reply buffer. A server that runs on a
PC (the hub, a simulator, a test bench) usually has its data in `String`s and `Vec`s instead. With
`no_alloc = false` its handlers take and give owned types:

| In the API             | `no_alloc = true` handler | `no_alloc = false` handler |
|------------------------|---------------------------|----------------------------|
| `String` / `&'i str`   | `&str`                    | `String`                   |
| `Vec<T>` / `RefVec`    | `RefVec<'_, T>`           | `Vec<T>`                   |
| `Foo<'i>` (a user type) | `Foo<'_>`                | `FooOwned`                 |
| `u8`, `Point`, ..      | the same                  | the same                   |

That holds for method arguments and return values, property values and their write errors, sink items, the result
of a stream function, stream items (`<name>_send`, `_push`) and the value of a deferred method's
`<method>_send_return`.

```rust
pub struct Server {
    devices: Vec<DeviceEntryOwned>,
}

impl Server {
    fn devices(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Vec<DeviceEntryOwned>> {
        Ready(self.devices.clone())
    }

    fn set_label(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        label: String,
    ) -> SetResult<()> {
        // ..
        Set
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        my_api :: Hub for super::Server,
        server = true, no_alloc = false, use_async = true,
    );
}
```

`ww api scaffold my_api --alloc` prints every handler with these signatures (see the
[CLI reference](../cli_reference.md)).

### Same bytes on the wire

A client can't tell the two kinds of server apart: an owned value is written into the reply buffer with the bytes
its borrowed counterpart gives. The server builds its replies in a fixed buffer, as a `no_alloc` one does, and writes
owned values with shrink_wrap's `BufWriter::ser_owned` / `write_owned`: the value goes through a `BufWriterOwned` that
continues from the writer's position (the bit in the open byte included), then its bytes are copied back and the
lengths it left for the enclosing value are handed on. `shrink_wrap::OwnedSer(&value)` is such a value as a
`SerializeShrinkWrap`, for generic code that wants one.

The `methods`, `properties` and `streams` test suites run against `no_alloc = false` servers, with a `no_alloc`
server of the same API next to each (`tests_common::Twin`): it gets every request too, and its reply and its events
have to be the same bytes.

### What stays borrowed

- `const` items: their values are Rust consts or `fn`s of the `<Root>Consts` trait, as in a `no_alloc` server.
- Framework types: `ValidIndices<'_>` of a resource array, the `TailBytes` a `[u8]` sink hands to `write_<name>`.

### What it costs

- An owned value is serialized into a growable buffer first and copied after: one allocation and one copy per value
  written. A `no_alloc` server writes in place.
- Owned types have no size bound (`MAX_SIZE`), so a `no_alloc = false` server has no `<METHOD>_REQUEST_MAX_LEN` /
  `<METHOD>_RESPONSE_MAX_LEN` consts, and an observed property with an owned type is read by `poll_properties` to be
  measured, like any value without a bound. A level of such properties gets its update type as
  `<Trait>UpdateOwned`.
