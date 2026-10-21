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
client → server   | "WWHB" | version: u8 = 2 | target: u8 | key_len: u16 LE | key: UTF-8 | trailer_len: u16 LE | entries |
  each entry      | tag: u8 | len: u8 | value |
server → client   | "WWHB" | version: u8 = 2 | status: u8 | hub_version_len: u8 | hub_version: UTF-8 |
then, both ways   | len: u32 LE | kind: u8 | payload |    len counts kind + payload
```

- `target` 0 is the hub's own API (no key), 1 a device by its key (`usb:c0de:cafe:<serial>`); a process serving a
  single device accepts any key, the client sends an empty one.
- The trailer (version 2, SRV-30) carries the client's versions, each value `| major: u32 LE | minor: u32 LE | patch:
  u32 LE |`: tag 1 its `ww_link`, tag 2 its `ww_client_server`. A reader takes the entries it knows and skips the
  others; a missing entry is unknown. Version 1 is the same without the trailer, as clients from before SRV-30 send
  it: a server reads both, and a client asks again with version 1 when a server answers status 3 to version 2.
- `status`: 0 ok, 1 no such device, 2 the device is held by another program, 3 preamble version not served, 4 the
  client's `ww_link` or `ww_client_server` version is not served ([Version skew](#version-skew-and-handover-srv-30)),
  5 the device is lent to another program for a while (SRV-32: wait and ask again). The server closes the
  connection after any status but 0, and the client's connect error says which, with the server's version
  (`Error::HubRefused`).
- The preamble is frozen: what is in it never changes, later fields only come as new trailer entries (a new tag), and
  a reader of any version can still read an answer up to the server's version string, so two programs built years
  apart can at least tell each other what they are.
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

- `info()` and `handover(socket)` are frozen: a client of any version can ask what hub runs and have it replaced by a
  newer one ([Handover](#handover-to-a-newer-hub)).
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

### Limits of the routing

A routed connection is a [device tunnel](#device-tunnels-srv-29), with that section's limits. Only USB devices are
routed so far; RTT and the other transports connect directly (a program reaches an RTT device through the hub with
`ClientConfig::hub_device("rtt:..")`). `connect_blocking()` routes when it runs on a multi-thread tokio runtime
(Python's is one); on a current-thread runtime it connects directly.

## Python and notebooks (TOOL-18)

The wheel (`wire_weaver_py`) carries `wire_weaver_client` with its `hub` feature and `ww_hub`, so Python follows the
same routing as a Rust program, with no separate `ww` install needed:

- `python -m wire_weaver.hub` serves the hub; `wire_weaver` registers `sys.executable`,
  `["-m", "wire_weaver.hub"]` at import time ([register_self_hosting_with](#starting-the-hub)'s Python side), so
  [`ensure_running`](#starting-the-hub) finds it the same way it finds a Rust program that called
  `ww_hub::main_if_hub()`: among the candidates it starts the newest it can find (`ww` on `PATH` included, if
  installed), asked `--hub-version` first. A bare `python -m wire_weaver.hub` without `WW_HUB_SERVE` set refuses
  and says so, since it is meant to be started by the client library, not run by hand (`ww hub serve` is that for
  a Rust-hosted hub).
- `connect()` takes `hub_device` (a device key straight through the hub, skipping enumeration and the other
  filters) in addition to the usual filters; `WW_HUB` / `WW_HUB_OPENS` route a plain `connect(vid_pid=..)` the same
  way they do for a Rust client ([The modes](#the-modes), [Repeated opens](#repeated-opens)).
- `wire_weaver.hub()` is a client of the hub's own API (`ww_hub_api`) through the dynamic client, starting the hub
  if none runs: `hub().info`, `hub().registry.devices()`, `hub().registry.clients()`, and
  `for e in hub().registry.events: ...` since a hub API stream is a `Stream` like any device's. It needs no API
  crate on disk: the hub, like any device, answers an introspection request.

Notebooks gain the most: `connect()`'s client meta kind defaults to `Notebook`, which allows a
[polite handover](#reaching-devices-through-the-hub-clt-39) from a direct holder by default, and a kernel restart
(a Python process exits and a new one takes its place) no longer drops the device, its steering or its
introspection — the new process reconnects through the hub in the time of a local socket round trip, not a cold
USB enumeration. Two notebooks, or a notebook and the `ww` CLI, share a board the same way.

`connect_blocking()` (what every Python call uses, since `pyo3` releases the GIL around it rather than running on
an async task) reaches a local socket or the hub the same way `connect()` does; `tests/hub_pytest_device` hosts the
ww hub and an in-process test device in one binary (no real hardware), so `wire_weaver_py`'s pytest suite
(`tests/test_hub.py`) covers the kernel-restart reconnect and two Python processes sharing one device without it.

## The `ww hub` command (TOOL-17)

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
`ww hub serve`; programs start it by themselves ([Starting the hub](#starting-the-hub)). `lend` is in
[The whole device](#the-whole-device-exclusive-access-and-lending-srv-32).
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
- Device keys: `usb:`, `rtt:` (with the `rtt` feature, `ww` has it), `inproc:<path>` (a device served in the
  hub's own process: simulators, tests) and `ipc:<socket path>` (a device another process serves on a local socket,
  [Device side](#device-side): a simulator or host-emulated firmware, which outlives the hub).

### What it costs

Every message crosses a local socket twice more (client → hub, hub → client), each a wake of another process. The
hub serves on one thread (`ww_hub::run_blocking`), so inside it a message is handed from task to task without waking
another thread. What that costs depends on how deeply the CPU sleeps between messages more than on the hub's own work:

- With a device in the hub's process, the hub adds about 35 µs per round trip on an idle PC.
- Over USB, the hub's own round trip to the device is as fast as a direct client's (480 µs on the b200 board,
  direct 510-600 µs), but the two extra wakes come from cores that went idle while the transfer was on its way: 16 B
  round trips are 10-25 % fewer on an idle PC, 3-13 % fewer while other programs keep the cores busy.
- Messages that don't wait for each other (stream data, a batch of requests) are not slowed down: throughput is the
  same as direct within the run-to-run spread, and so is a host → device stream once enough messages are in
  flight (the benchmark's host → device stage keeps 64 at most, so it shows the latency too).

`tests/hub/examples/hub_loopback.rs` runs `ww usb-loopback`'s test directly and through a hub on a runtime of its
own, in alternating rounds, and compares the medians:

```sh
cargo run --release -p hub --example hub_loopback -- usb:c0de:cafe:<serial> [seconds] [packet size|max] [rounds]
```

Medians of 7 rounds of 2 s per stage on an idle PC (omarchy-m1, Apple M1 under Asahi Linux, load 2-6), direct → hub:

| Device                          | Size  | Round trips/s        | Host → device         | Device → host          |
|---------------------------------|-------|----------------------|-----------------------|------------------------|
| donguru (USB FS)                | 16 B  | 3373 → 2883 (−15 %)  | 8859 → 7440 msg/s     | 26118 → 26111 msg/s    |
| donguru (USB FS)                | max   | 350 → 345 (−1 %)     | 735 → 767 kB/s        | 537 → 535 kB/s         |
| b200 (USB FS)                   | 16 B  | 2461 → 1886 (−23 %)  | 12554 → 10087 msg/s   | 38429 → 38614 msg/s    |
| b200 (USB FS)                   | max   | 112 → 131 (+17 %)    | 362 → 457 kB/s        | 547 → 552 kB/s         |
| B135 (USB HS)                   | 16 B  | 6252 → 4474 (−28 %)  | 16701 → 16013 msg/s   | 159730 → 159029 msg/s  |
| B135 (USB HS)                   | max   | 1060 → 1088 (+3 %)   | 11.0 → 12.7 MB/s      | 18.9 → 20.4 MB/s       |
| in the hub's process            | 16 B  | 36771 → 16131 (−56 %) | 25620 → 25489 msg/s  | 698446 → 667606 msg/s  |

Single runs swing by ±15 % on a PC that does other work: compare medians of several rounds.
## Reconnect after a hub restart (CLT-40)

To a program, its hub going away looks like an unplugged device: the socket ends. A connection through the hub (routed
by `connect()`, or `ClientConfig::hub_device`) connects again by itself instead, and the program goes on:

- **What triggers it.** The link through the hub goes: the hub exited, crashed or was killed, handed over to a newer
  one ([below](#handover-to-a-newer-hub)), lent the device to another program (SRV-32), or stopped answering.
  A disconnect the program asks for doesn't (`disconnect()`, dropping the client).
- **Calls.** Calls in flight fail with `Error::Disconnected`, and so does every call until the link is back, at
  once (CLT-36). None is sent again by itself: a call may have reached the device before the link went, and calls
  aren't idempotent in general.
- **Reconnecting.** The client tries at once, then with backoff (10 ms doubling to 1 s): through a hub again, starting
  one when none runs (in `HubMode::Auto` as if `Prefer`, see [the modes](#the-modes)), with link setup and the version
  checks as on the first connect. A device with another API than before ends it.
- **Streams.** Counted and reliable streams wait and resume from their cursor (`StreamEvent::Resumed`, gaps if the
  device didn't keep everything), observed trait levels are subscribed again (`ObserveEvent::Resumed`), sinks are
  registered again and a credit sink's session goes on: the same path as
  [`disconnect().keep_streams()` and `Commander::resume_streams()`](../api/stream_reliability.md). The device keeps its streams'
  sessions while the hub is gone, so nothing is lost but what didn't fit its store. Plain streams (without `#[counted]`
  or `#[reliable]`) end with the link, as on an unplug: open them again after `Reconnected`.
- **Giving up.** After `ClientConfig::hub_reconnect(Some(give_up))` without a link (`DEFAULT_HUB_GIVE_UP`, 60 s) the
  client stops: the waiting streams end and calls keep failing. Time the device is lent doesn't count: the hub said it
  comes back (preamble status 5, or the tunnel closed with `DisconnectReason::Other(1)`), and while it is lent no hub
  is started. Versions the hub doesn't serve end it at once. `hub_reconnect(None)` turns it all off.

The app hears about it from `Commander::connection_events()`, a broadcast receiver:

```rust
let mut events = device.cmd.connection_events();
while let Ok(event) = events.recv().await {
    match event {
        ConnectionEvent::Disconnected { reason, reconnecting } => show_gap(reason, reconnecting),
        ConnectionEvent::AttemptFailed { attempt, error, retry_in } => {}
        ConnectionEvent::Reconnected { gap, attempts, resumed } => show_back(gap),
        ConnectionEvent::GaveUp { error } => break,
        _ => {}
    }
}
```

`GapReason` says why: `Lost(why)` (the socket ended, an IO error, the peer timeout), `HubHandover`, `Lent`,
`Disconnect(reason)` (the hub or device ended the link for another reason). While the client reconnects by itself,
`Commander::reconnect()` is refused; after `GaveUp` it works as after any disconnect.

Measured on Linux (`tests/hub_reconnect`, `kill.rs`): with calls in flight all the time and a counted stream open,
the hub killed with `SIGKILL` twice; each time the client was back in about 90 ms, the new hub started by the client
itself, the stream went on from where it was, the calls in flight failed with `Disconnected` and none reached the
device twice.

## Version skew and handover (SRV-30)

Programs on one PC are built at different times: `dg` with one WireWeaver release, the rePort GUI with another, one
hub for both. Three layers keep that working, each under WireWeaver's own [evolution rules](../evolution/rules.md#the-ww-hub-and-version-skew):

1. **The preamble** is frozen ([Wire format](#wire-format)): any client and any hub can exchange their versions and a
   clear refusal.
2. **The envelope.** The hub reads only what is around a user API: `ww_link` messages and the envelope of
   `ww_client_server` requests and events. A client says its `ww_link` and `ww_client_server` versions in the
   preamble, and the hub serves every version its build is compatible with by Cargo's SemVer rule (the same major;
   for 0.x the same minor), so a newer hub keeps serving older clients. When a release breaks `ww_link` or
   `ww_client_server`, the hub keeps the previous major's code for one more major and lists it in its window
   (`ww_hub/src/window.rs`), as a device keeps an old protocol. The device's versions only have to match the hub's
   build: the hub talks to the device itself. The user API is end to end between client and device, checked against
   the device as without a hub (CLT-8).
3. **The hub's own API** (`ww_hub_api`) evolves like any API: a client newer than the hub gets `Unimplemented` for
   methods the hub doesn't have yet.

### A refusal

A hub that doesn't serve a client's versions answers status 4 and closes. The client's `connect()` fails with
`Error::HubRefused { status: 4, .. }`, naming both sides and what to do, by which one is older:

```
…/hub.sock: the ww hub 0.6.0 doesn't serve this program's ww_link 0.5.0 and ww_client_server 0.2.0: rebuild this
program with WireWeaver 0.6.0 or newer (it is built with 0.5.0), or stop the hub (`ww hub stop`) and connect directly
with WW_HUB=off while it is stopped (ww hub 0.6.0)
```

The client doesn't fall back to opening the device directly then: the hub holds it, the open would only find it busy.

### Handover to a newer hub

Only strictly newer replaces older, so the hub on a PC converges on the newest one and never flaps between two (the
mistake of adb, where two versions kill each other's server forever):

1. A program that can host a hub newer than the one running (`ww_hub::main_if_hub()` registered its own; it costs no
   process to find out, so every connect checks) takes `hub.start.lock`, as for any start, and starts itself with
   `WW_HUB_SERVE` and `WW_HUB_HANDOVER` set.
2. The new hub (`ww_hub::Bound::take_over`) listens on a socket of its own (`hub.<pid>.sock`) and calls
   `Hub::handover(socket)` on the running one.
3. The old hub asks the new socket for its version (the preamble answer) and refuses one that isn't strictly newer
   (`HandoverError::NotNewer`). Otherwise it stops accepting (its socket file goes at once, so clients find no hub
   rather than one that doesn't answer), sends `Event::Handover` to the registry's listeners, closes every tunnel and
   API connection with `DisconnectReason::HubHandover`, closes its devices, and exits (`Exit::Handover`), releasing
   `hub.lock` once its connections sent what they still had (500 ms at most).
4. The new hub takes `hub.lock` and moves its socket into place: a rename on Unix (the bound socket stays, its name
   changes), the pipe created anew on Windows once the old one's instances closed.
5. The clients reconnect to it by themselves ([CLT-40](#reconnect-after-a-hub-restart-clt-40), `GapReason::HubHandover`),
   and the program that asked goes on with the new hub. A hub that can't hand over (built before SRV-30) answers
   `Unimplemented`: the new one exits and the old one keeps serving.

The start lock is held through all of it, so a client reconnecting in the gap waits for it instead of starting a third
hub. Tested in `tests/hub_reconnect` (`handover.rs` with clients connected, `replace.rs` across processes); macOS and
Windows compile only so far.

## Claims on resources (SRV-34)

Several programs share a device through the hub; sometimes one of them needs a part of it to itself for a while (a
test sequence that reconfigures a CAN bus, a firmware update). It *claims* that resource through the hub's
`claims` group, and until it lets go, the hub answers other programs' calls and property writes there itself, with
`Common(InUse)` naming the holder, without reaching the device. Which resources can be claimed, and in which mode,
is in the device's API (`#[claim(..)]`, `#[shares(..)]`, `#[label(..)]`, see [Metadata](../api/metadata.md)); the
root of a device always can, exclusively.

```rust
let meta = ClientMeta::new().purpose("flashing B135 #4");
// one program: its hub client and its device client share one client instance
let hub = HubClient::config(|c| c.hub_api().client_meta(meta.clone())).connect().await?;
let board = Board::config(|c| c.hub_device(key.clone()).client_meta(meta.clone())).connect().await?;

// `can[0]`: the path of resource ids from the root, array indices included, as in a request
let id = hub.claims().claim(key.clone(), vec![UNib32(3), UNib32(0)], "bitrate sweep".into(), Some(600)).call().await?;
board.can(0).set_bitrate(500_000).call().await?; // goes through: this instance holds the claim
// ... another program's `can(0).send(..)` gets Err(Common(InUse { by: "sweep pid 4711" }))
hub.claims().release(id).call().await?;
```

- `claim(key, path, purpose, ttl_s) -> Result<u32, ClaimError>`, `release(id)`, `claims(key)` (every claim on the
  device: path, mode, the resource's label, purpose, holder, since, expiry). `registry().events()` says `Claimed` and
  `Released`. The hub opens the device if it doesn't hold it yet and reads its API from the introspection data (once,
  cached with it), since it never links the device's API crate.
- `ClaimError`: `UnknownDevice`, `ApiUnknown` (no introspection: only the root can be claimed), `BadPath`,
  `NotClaimable` (no `#[claim]` on it nor on its trait: a typo can't take a whole board), `InUse` (another program
  holds this resource), `Overlap` (another program holds a resource above or below it, or one that `#[shares]` a pin
  or peripheral with it). Each names the claim in the way.
- Who holds it: the client instance (the instance id of its `ClientMeta`, CLT-41), as the principal it connected as
  (today always the local user; remote principals come with SRV-33/SRV-37). Every connection of that instance, to
  the hub's API and to devices, may use the resource, so a program gives its hub client and its device clients clones
  of one `ClientMeta`. A client that sends no client meta (built before CLT-41) holds claims for that one connection.
  One instance may nest and overlap its own claims.
- A claim goes when it is released, when its instance's last connection to the hub closes (a crash or `kill -9`
  frees it), or after its ttl.

What is refused, for programs other than the holder, at or under the claimed path:

| Mode | Calls, property and sink writes | Reads, streams, observing |
|---|---|---|
| `exclusive` | refused | open |
| `shared` | refused for programs holding no claim on it; several programs claim it together | open |
| `observe` | refused | refused |

- The answer is typed where the item has an error type: a method returning `Result<_, E>` gets `Err(E::Common(InUse
  { by }))`, a property write with an error type gets that error, built from the device's API with the dynamic
  serializer, so the program's own client decodes it as a normal `CallError::Device`. Without one (`fn ping() ->
  u8`), the error event says `in use by <program> pid <pid>` (`ErrorKind::UserStr`).
- Requests addressed by trait (global paths) are placed with the device's API; multi-requests (`MultiCall`,
  `MultiWrite`, ...) are refused when a claim is at, above or under their path. A request the hub can't read (an
  envelope newer than the hub) is refused like a call anywhere on the device while other programs hold claims on it,
  and passes as it is when nobody does.

### Hub claims and device claims

A device can have claims of its own, between functions inside the firmware (rePort's `b200_board`: Claim / Plan of
pins and functions, so a UART and a pin test can't both drive PA9). The two layers don't know each other:

- The hub layer is between programs: which client may command this subtree. It comes first: another program's claim
  is answered by the hub, naming the program.
- The device layer is between functions: what the firmware may do with its pins and peripherals right now. A request
  that passed the hub reaches the device, whose own checks may answer `InUse` or `Refused`, naming the function.
- Neither takes the other: a hub claim doesn't make device-level Claim / Plan calls, the program holding it makes
  them as usual (and a device claim doesn't show up in the hub's `claims`). A program connected directly (no hub)
  holds the whole device, so only the device layer applies.

## The whole device: exclusive access and lending (SRV-32)

A firmware update or a settings write needs the whole device to itself: another program's call in the middle of it
could do harm, and the hub's own clock steering would disturb the device while it reboots into a trial. That is a
claim on the device's root path (an empty path, always claimable, exclusively): every other program's calls and
writes anywhere on the device are refused with `Common(InUse)` naming the holder, their reads, streams and observing go
on, and the hub doesn't steer the device's clock while it is held (SRV-31). It is an ordinary claim otherwise: owned
by the client instance, gone with its last connection or its ttl, listed by `claims(key)`.

```rust
let meta = ClientMeta::new();
let board = Board::config(|c| c.client_meta(meta.clone())).connect().await?;
// `Some` when a hub of this user holds the device: this program reaches it through the hub
let exclusive = wire_weaver_client::hub::Exclusive::take(&key, &meta, "flashing B135 #4").await?;
// ... the update; a reconnect after a reset made with `meta` again keeps it
if let Some(e) = exclusive {
    e.release().await?;
}
```

`hub::Exclusive::take(key, meta, purpose)` takes it only when a hub runs and holds the device; otherwise the program
holds the device directly, which is exclusive by itself, and gets `None`. `meta` has to be the `ClientMeta` the
device connection was made with: the claim belongs to its instance id, so that connection's calls go through. Another
program's exclusive in the way is an error naming it (`usb:..: in use: dg pid 4711 (flashing), lent`). `ww dfu
reboot | flash | confirm | revert` take it this way ([`ww dfu`](../cli_reference.md#ww-dfu)); `flash` keeps it across
the device's reset into the trial, since the claim is by device key and the program's hub connection stays up.

### Lending to a program that isn't WireWeaver

probe-rs, dfu-util or a serial terminal can't go through the hub: they open the device themselves, and fail while
the hub holds it. `ww hub lend DEVICE -- CMD` asks the hub to let it go for one command:

1. `Claims::lend(key, purpose, ttl_s)` takes a root claim marked lent (refused like any claim when another program
   holds one on the device), then the hub closes the device: every tunnel to it gets `Disconnect` with
   `DisconnectReason::Other(DISCONNECT_LENT)` (`ww_local_socket`), the hub's event loop to the device exits and lets
   the USB interface or probe go. `lend` answers only then, so `CMD` can open the device right away.
2. While it is lent, the hub doesn't open the device: a new tunnel to it gets the preamble answer `Status::Lent`,
   `take` and claims of others are refused naming the lend. `registry().events()` says `Lent { key, id }`.
3. `ww hub lend` runs `CMD` with the terminal. Ctrl+C reaches `CMD` (the terminal sends it to the whole foreground
   process group, a console to every process on it); `ww` only keeps running to give the device back after, and
   exits with `CMD`'s exit code (128 + the signal on Unix when a signal ended it).
4. Releasing the claim returns the device (`Returned { key }`): the hub opens it again if it held it before (kept
   too if it was `take`n), so the programs that were connected find it open when they reconnect. A borrower that
   crashes returns it the same way, with its last connection; a ttl ends a lend too, checked as the hub goes.

`hub::Lend::take(key, purpose)` is the same from a program; without a running hub it is `None` and the command just
runs. Reconnecting clients wait while the device is lent and come back when it is returned (CLT-40).

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
