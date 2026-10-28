## Unreleased

### ⚠️ Breaking

- `quantity::DynUnit::Named::dim` is a `ww_si::DimOwned` (angle, information and counted bases, STD-60), `DynUnit::dim()` answers `Option<Dim<'_>>`: compare with `same` or through `exps()` / `counted_bases()`.
- `multi::call_array` / `call_array_blocking` take any `CallPath` builder: the generic parameters are `<C, F>` instead of `<T, F>` (a turbofish `call_array::<u8, _>` drops the `u8`), and they answer `Vec<C::Output>`, which is `Vec<Result<T, ErrorOwned>>` as before for methods without `Result` (CLT-48). The hidden `multi::CallPath` trait has `Item` (wire value) next to `Output` and an `output` method.
- Default-priority traffic follows one rule (CLT-50): a frame goes out when it is full or when the device's accumulation window is over. The flush as soon as the tx task had nothing else waiting (CLT-37, for windows under 1 ms) is gone: a lone request now waits for the window (250 µs on USB HS, 1 ms on FS with `UsbTimings::fs_higher_speed`). Mark a request that must not wait `.urgent()`. `internal` users: `TxInput::Idle` and `TxCore::wants_idle_flush` are removed.
- `evolution` is a re-export of the new `wire_weaver_evolution` crate (EVO-11), where the checker lives now, together with `layout` and the type rules of `compat`; paths like `wire_weaver_client::evolution::compare` keep working. `evolution::Report::breaking` is a `Vec<Finding>` (the rule broken and the message) instead of `Vec<String>`: use `finding.message`, or its `Display`, which adds the rule.
- `Error::DeviceNotFound` has the new field `over_rtt` (TOOL-20): the message says `No debug probe found` or lists the probes that didn't match instead of talking about WireWeaver USB devices; code building the variant sets it. The filters in the message name the RTT target, and the filters after it as `probe ..`.
- `clock_sync::Fit` has two more public fields, `max_rtt` (the longest round trip of the samples used) and `rate_sensitivity`: code that builds a `Fit` with a struct literal adds them (`0` / `0.0` for a fit without a measured slope).
- `adopt::LinkSnapshotOwned` is split into a medium-independent and a medium part (CLT-44): `adopt::HandoverOwned { client_version, key, link, medium }` with `link: LinkResidualOwned` (token, `DeviceInfo`, seqs, requests in flight, streams, observed levels, `full_only`, ping timers) and `medium: MediumResidualOwned { kind: MediumKindOwned, bytes }`, bytes that only the medium of that kind decodes (for USB its endpoints and the framers' carry, which were `MediumOwned`, `tx_frame`, `rx_partial`, `rx_staged`, `rx_next_frame`). `MediumOwned`, `FrameAtOwned` and `PartialOwned` are gone from `adopt`. `internal::Adopted::new(link, medium, tag, forward_timeout)` takes the two residuals instead of a snapshot. A hub and a holder of different minors never adopted each other's links, so deployed hubs are not affected beyond that.
- A direct connection steers its device's clock onto the PC's UTC by default (CLT-46), when the device serves
  `ww_timebase::Clock` with `host_sync`: a task of the connection reads `now()` a few times a second and sends steer
  points. `ClientConfig::clock_sync(false)` (or `WW_CLOCK_SYNC=off`) keeps the old behaviour; a program that steers
  with a `ClockSync` of its own should turn it off. A connection without introspection data (`traits_only`) tries
  `Clock::now()` once to find out. New default feature `clock_sync` (dependencies `ww_timebase`, `ww_date_time`).
- `internal::Command::Adopt { adopted }` is new (CLT-44): an exhaustive match needs the arm.
- `EventLoopExitReason::HandedToHub` is new (CLT-43): an exhaustive match needs the arm. `holders::Holder` has the new
  public field `handover` (the holder's socket for a polite handover, empty if none): code building a `Holder` sets it.
- RTT takes its lock-file claim in the event loop's `connect` (on the tokio runtime) instead of on the probe's IO thread,
  still before the probe is opened; both halves of a USB, RTT or `ipc_exclusive` link keep the claim, so it goes once
  both closed the medium.

- `EventLoopExitReason::DisconnectFromDevice` carries the `DisconnectReason` the device (or the ww hub) sent.
- `Command::Connect` has the new field `keep_streams_on_loss`: when the link goes rather than being disconnected on
  request, counted and reliable streams, observed levels and sinks stay for a resume as after
  `disconnect().keep_streams()` (set by the reconnect through the hub, CLT-40); `false` keeps today's behaviour.
- `Selected::Device` (internal) and the `Error` enum: `Error::HubRefused { target, status, why, hub_version }` is new,
  a refusal of the hub's preamble answer (before, a refusal became a `ConnectFailed` string); exhaustive matches need
  the arm. A local socket connection (`ipc_path`, `hub_api`, `hub_device`) opens its tunnel before the event loop
  starts, so a refusal fails `connect()` as this error.

### 🐛 Fixes

- A `MultiCall`/`MultiRead`/`MultiWrite` tuple may name the same resource more than once, e.g. two `i2c_write` calls (CLT-49): it failed with `Error::MultiReq("resources are identical ..")`, now the id repeats in the request's index list and the answers come in tuple order. No device change needed.
- An accumulation window of `W` costs at most `W` (CLT-53): the tx task waited for it with tokio's timer, which rounds up to whole milliseconds, so a 1 ms window cost ~2.2 ms per request. It now waits on a fine timer (a thread per process for deadlines under 2 ms away), within tens of µs.
- RTT: the RAM scan attaches to every control block it finds and takes the one with `ww_up` / `ww_down`, and retries within its window while the one found has none: probe-rs takes the first `SEGGER RTT` of a RAM region, which after a flash can be the previous firmware's, left in RAM (Nucleo G0B1RE, 23 Oct 2026: `no channel named 'ww_up'` with garbage names). `hub::take` waits for the hub to open the device up to 15 s instead of the 1 s default request timeout: over RTT the open (RAM scan, link setup) takes longer, and `ww hub take rtt:..` said `Client(Timeout)` although the hub had taken it. A probe another program claimed (`ProbeCouldNotBeCreated(Usb(..))`, e.g. `probe-rs attach` on an ST-LINK) now names its holder from `/proc` and keeps the USB cause instead of only `The debug probe could not be created`.

- Dropping the last `Commander` waits until the event loop closed the transport (a few ms), 50 ms at most, instead of always blocking 50 ms; with the snapshots parsed one at a time (`wire_weaver_snapshots`), a short program through the ww hub (`ww hub status`) connects, calls and disconnects in about 15 ms instead of 150-300 ms. Found by a `ww hub` test hitting its 3 s limit on a loaded PC.
- `holders` lock files are one per device key (CLT-45): lower-case letters, digits and `-` stay, every other byte of the key is `_xx` (was `_` for any of them, so `usb:..:a:b` and `usb:..:a_b` shared a file and a crafted USB serial could make another device look held). A name over 200 bytes, and a handover socket path too long for a Unix socket, is `_h` and the key's SHA-256 (was FNV-1a for the socket). A held lock file whose content names another key reads as an unknown holder. Lock files of programs built before this change aren't seen by programs built after it (holders are unreleased so far).
- `local_socket::device::device_medium` accepts clients in a task of its own: `wait_connected` was not cancel-safe, and a server waiting on it next to a timer dropped a client in the middle of its preamble (the client saw `Connection reset by peer` or `Broken pipe`). The next client still waits in the backlog until the current one leaves.
- A request whose bookkeeping reached the receive task after it exited (the link going down while the request was sent) fails with `Error::Disconnected` instead of `RxDispatcherNotRunning`.
- The other side closing the medium in answer to our `Disconnect` ends the event loop as asked (`DisconnectCommand`) instead of as a lost link: no `rx exiting with: Transport specific error` ERROR log at the end of a short `ww hub ...` call, and streams aren't kept for a reconnect after a plain `disconnect()`.
- A call (or any command) while no event loop runs, after the link went down or `disconnect().keep_streams()` and before `Commander::reconnect()`, fails at once with `Error::Disconnected` instead of waiting in the command channel until someone reconnects (CLT-36): a keeper task holds the channel between event loops and answers what comes in; `reconnect()` takes the channel from it and gives it back.
- `connect_blocking()` reaches a local socket (`ClientConfig::ipc_path` / `hub_api` / `hub_device`, TOOL-18): its interface loop had no `InterfaceKind::Ipc` branch, so every blocking connect through a local socket or the ww hub failed with "no devices found" even with one answering; `wire_weaver_py`'s `connect(hub_device=...)` and `hub()` are the first callers.
- A `LoopbackTest` sent while link setup still runs waits for the link instead of failing with "not connected".
- `holders` (CLT-38) find the runtime dir with `ww_local_socket::runtime_dir()`, the hub's own: `/tmp/ww-<uid>` on Linux without `XDG_RUNTIME_DIR` (was an error), `WW_RUNTIME_DIR` honoured, a symlinked dir refused; the `rustix` dependency is gone, and the crate builds without warnings on Windows and macOS.

### 🚀 Features

- `quantity` reads the units of the new `exact_units` crate (STD-61): introspection names the markers, `Dim`, `SIExpr` and `Fixed` by `exact_units` (any version) or `ww_si` 0.2 and later, both are units. `DynUnit::Sqrt` is the square root of a unit (`units::Sqrt<Hertz>`, shown `√Hz`), also from an `SIExpr::Sqrt`.
- Methods returning `Result<T, E>` go into `MultiCall` tuples and `call_array` (CLT-48), mixed with methods without `Result`: each position answers what its single `call()` would, `Result<T, CallError<E>>` (`Device` checked against the method's `#[errors]` list, `Unimplemented`, `Client(Error::RemoteError(..))` for the index's own error such as `BadIndex`). Methods without `Result` answer `Result<T, ErrorOwned>` as before. No device change needed. See `docs/api/multi_requests.md#methods-returning-result`.
- Request priority (CLT-50): `.priority(p)` and `.urgent()` on `PreparedCall`, `PreparedRead`, `PreparedWrite` and `PreparedStreamOpen`, `Commander::set_default_priority` / `default_priority`, `ClientConfig::default_priority`; `Priority` is re-exported. An urgent request (`Priority::URGENT`, 8, and up) is flushed at once instead of waiting for the accumulation window, and the device answers it at once too. A multi request goes at the highest priority of its items. `explain_request` shows the request's `priority` nibble.
- The runtime compat check (`compat`) lets a device send an enum variant the client doesn't know when the client's enum has the `#[future_version]` catch-all (EVO-10): it reads it as `Unknown(n)` instead of the resource being marked incompatible; a moved variant is reported as `variant X moved`.
- With the `rtt` feature: `list_probes()` (`ProbeInfo`: VID, PID, serial, probe-rs's name) lists the debug probes without opening them, `rtt_chips(prefix)` the probe-rs chip names, `rtt_chip_known(name)` whether probe-rs knows one, for `ClientConfig::rtt` (TOOL-20, used by `ww --rtt` and its shell completion).

- `clock_sync::Fit::rate_bound_ppb(resolution_ns)`: how far the fitted rate can be from the device's true rate at most, a bound that holds whatever the load (each sample's host time is within half its round trip of the device's reading), `None` for a slope from the rate cache.
- The event loop has one transport slot (`event_loop::medium`, an enum over this crate's media, boxed for others) instead of each live medium's `Live<Device, Hub>` halves: a live handover's steps are the medium's in the slot (`Transport::pause`, `let_go`, `take_back`, `switch`, the claim), and `medium::relink` puts another medium's halves in its place, from any medium to any other (CLT-44's tunnel to the hub now; the foundation for handing WS and UDP links over). The USB medium checks that an adopted link was on the interface and endpoints it opened.
- Live link handover to the ww hub on Linux (CLT-44): a direct holder asked to hand its device over (CLT-43's
  request) hands the hub its live link instead of releasing the device, when the hub's `wire_weaver_client` has the
  same minor version. The event loop pauses the link (OUT transfers sent, IN cancelled and reaped into the framer),
  sends a `LinkSnapshot` to `Adopt::prepare`, releases the interface keeping the device open, has the hub open it
  with `Adopt::commit` and goes on through a tunnel with the same seqs, requests in flight, streams and observed
  levels: no call fails, no `ConnectionEvent`, the device sees no reconnect. The hub refusing falls back to CLT-43;
  the hub going before it holds the device leaves the holder going on directly; the interface taken by another
  program in the gap ends the link as `HandedToHub` and reconnects through the hub. USB through nusb (Linux) and the
  simulated USB device; other media and OSes hand over by release and reopen, and so does a holder with
  `WW_LIVE_HANDOVER=off` (`hub::LIVE_HANDOVER_ENV`). On a real board: 17 ms median with 20 requests in flight, no
  call failed.
- `adopt` module (CLT-44): `LinkSnapshotOwned` (`encode`, `decode`, `check`, `MAX_SNAPSHOT_LEN`), `MediumOwned`,
  `FrameAtOwned`, `PartialOwned`, `TraitIdOwned`, `CLIENT_VERSION`, `same_minor`, `random_id`;
  `PreparedConnection::adopt(internal::Adopted, sink)` opens a device going on with an adopted link (the hub's side),
  the answers to the other program's requests in flight going to the forward sink from the first frame on.
- `usb_sim::UsbSimControl` for tests: `hold_claims`, `claims_waiting`, `claims_dropped` (a claim whose host went
  while it waited is never granted), `grab` / `ungrab` (another program holds the interface), `held`.
- Simulated USB device on a local socket (`usb_sim`, feature `local_socket`): `usb_sim::device` / `device_framed`
  serve one on a `ww_local_socket::Listener` for `ww_device::FramedTx` / `FramedRx` (`UsbSink`, `UsbSource`), hosts
  reach it with `ClientConfig::usb_sim_path` (holders key `usbsim:<path>`, the ww hub opens it under the same key).
  Unlike `in_process::usb_device` it outlives its hosts as a USB device does: one host holds the interface at a time,
  IN packets wait while nobody does, and the device's link doesn't notice a host releasing and another claiming, so a
  live link handover (CLT-44) can be tested across processes.
- `clock_sync` module (feature `clock_sync`, default; CLT-46): the code of `ww_clock_sync` (`ClockSync`, `Estimator`,
  `DeviceTime`, `HostClock`, `RateCache`, `Options`, `Status`, `steerer`) moved here, with `serves_host_sync(bundle)`
  and `rate_key(key)` from the hub. Every direct connection steers by itself: `ClientConfig::clock_sync(on)`,
  `ClientConfig::clock_sync_options(Options)`, `clock_sync::ENV` (`WW_CLOCK_SYNC`); `Commander::clock_sync()` gives a
  `ClockSteering` (`phase()`: `Starting`, `Steering`, `Paused`, `ThroughHub`, `Unsupported`, `Failed`; `status()`,
  `watch()`, `set_paused()`). Off while the connection goes through the ww hub (the hub steers), on again after a
  reconnect that reaches the device directly. The task holds the connection weakly, one round of calls at a time,
  so it never keeps it open. The rate cache is picked by the device's serial (USB, RTT), the steerer is the
  connection's `ClientMeta`.
- Polite handover to the ww hub (CLT-43): a program holding a device directly hands it to the hub when another
  program wants it, and goes on through the hub after a short gap. `ClientConfig::allow_handover(bool)`, by default on
  for client kinds `Cli`, `Notebook` and `Test`, off for `Gui`, `Service` and the rest, in effect where the holder can
  reconnect through the hub (`connect()`, `hub_reconnect` set, `HubMode` not `Direct`, feature `hub`). The holder
  listens on `<runtime>/ww/holders/<key>.sock` (user-only, its lock file names it: `handover = <path>`), lets go of the
  device on a `HandToHub` request and reconnects through the hub on the CLT-40 path:
  `ConnectionEvent::Disconnected { reason: GapReason::HandedToHub, .. }`, then `Reconnected`, streams resumed. One
  that doesn't allow it answers no with its client metadata. `connect()` (USB, RTT, `ipc_exclusive`) in every mode but
  `Direct`, and `hub::take`, ask a direct holder first when a hub runs or can start; a refusal fails naming the holder
  and its kind. Release and reopen; tested on Linux between two processes (gap ~5 ms on a simulated device).
- `ClientConfig::ipc_exclusive()`: a device on a local socket that takes one host at a time (a simulated USB device)
  is held like one: a lock file `ipc:<path>` (CLT-38, a second program is told who holds it), hub routing without the
  open counter (CLT-39) and polite handover (CLT-43). Off by default, so a process serving several connections stays
  reachable by all of them.
- `connect()` routes RTT probes through the hub too when the hub holds them, their holder hands them over, or in
  `HubMode::Prefer` / `Required` (the open counter still counts USB only).

- `hub::ensure_running(true)` replaces a running hub that is older than this program's own (registered by `ww_hub::main_if_hub()`, no process spawned to find out) under `hub.start.lock`: it starts itself with `hub::HANDOVER_ENV`, and the new hub takes over (SRV-30); a failure leaves the running hub in place. Only strictly newer replaces older, so it converges on the newest hub.
- The local socket preamble carries this program's `ww_link` and `ww_client_server` versions (SRV-30), and is sent again in the first layout to a hub that answers it doesn't read the second. A hub that doesn't serve them fails `connect()` with `Error::HubRefused { status: 4, .. }` naming both versions and the fix (rebuild with the hub's WireWeaver, or update the hub, or `WW_HUB=off` while it is stopped); routing doesn't fall back to a direct open then, and a reconnect gives up.
- Reconnect after a hub restart (CLT-40): a connection through the ww hub (routed by `connect()` or
  `ClientConfig::hub_device`) connects again by itself when its link goes: the hub exited, crashed, was killed,
  handed over to a newer one, or lent the device for a while. The client starts a hub if none runs (in `HubMode::Auto`
  as if `Prefer`), redoes link setup, resumes counted and reliable streams, observed levels and sinks
  (`resume_streams`), and tries with backoff (10 ms doubling to 1 s) until `ClientConfig::hub_reconnect(give_up)`
  without a link (`DEFAULT_HUB_GIVE_UP`, 60 s; time the device is lent doesn't count; `None` turns it off). Calls in
  flight fail with `Error::Disconnected`, as does every call until the link is back; none is sent again by itself.
  `Commander::connection_events()` tells the app: `ConnectionEvent::Disconnected { reason: GapReason, reconnecting }`,
  `AttemptFailed`, `Reconnected { gap, attempts, resumed }`, `GaveUp` (the waiting streams end). `GapReason`: `Lost`,
  `HubHandover`, `Lent` (`DisconnectReason::Other(1)` from the hub, preamble status 5: wait, don't start a hub),
  `Disconnect(reason)`. `Error::is_lent()`. Tested in `tests/hub_reconnect`.
- A device through the user's own hub (`hub_device(key)`, no path) starts the hub when none runs (feature `hub`),
  except with `HubMode::Direct`.

- `hub::Exclusive` (SRV-32, feature `hub`): `take(key, meta, purpose)` claims the whole device through the user's hub when it holds the device (`None` otherwise), with the `ClientMeta` of the program's device connection, `release()`; `hub::Lend::take(key, purpose)` has the hub lend the device to a program outside it; `take_at` of both for a hub at another socket; `hub::describe_claim`.
- Hub routing in `connect()` (CLT-39), feature `hub`: `ClientConfig::hub(HubMode)` (`WW_HUB` when not set: `auto`, `prefer`, `required`, `off`). `Auto`, the default, opens a USB device directly unless the hub already holds it (its registry says so) or the device was opened often lately: the open counter in `<runtime dir>/ww/opens/<key>` trips at 3 opens within 60 s (`ClientConfig::hub_opens(OpenCounter)`, `WW_HUB_OPENS=N/T`), a shell-launched client (kind `Cli`) counting 1, other kinds 1/4. `Prefer` goes through the hub, started if needed, and directly when none can start or it can't serve the device; `Required` fails instead; `Direct` never uses it and says so when the hub holds the device. Without the feature every mode but `Required` connects directly. `connect_blocking()` routes on a multi-thread tokio runtime only.
- `hub::ensure_running(start)`: the user's hub socket once the hub answers there; with `start`, a hub is started under `hub.start.lock` when none holds `hub.lock` (two programs starting at once get one hub), a stale socket file removed, the newest hub executable started detached (`WW_HUB_EXE`; else this program if `ww_hub::main_if_hub()` registered it, `ww` next to it, `ww` on `PATH`, each asked `--hub-version`), and its socket polled with the preamble for 2 s (no inherited pipe, the same on every OS). The hub's stderr goes to `<runtime dir>/ww/hub.log`. `hub::candidates()`, `hub_running()`, `register_self_hosting()`, `record_open()`, `take(key)` (`ww hub take`), `key_of(&DeviceInfo)`.
- `hub::register_self_hosting_with(version, exe, args)` (TOOL-18): a self-host that isn't the current executable run with no arguments, e.g. `wire_weaver_py` registers `sys.executable`, `["-m", "wire_weaver.hub"]`, since a bare Python interpreter doesn't serve the hub. `Candidate` carries the `args` to start it with, passed to both the `--hub-version` check and the detached spawn.
- `hub::HubClient`, the typed client of `ww_hub_api`, moved here from `ww_hub` (which re-exports it), so `connect()` can ask the hub's registry.
- Holder lock files (CLT-38) carry the client's metadata (CLT-41): `kind`, `user`, `host`, `label`, `purpose`, `instance_id` lines, and `program` from the meta; `Holder` has these fields (empty when an older program wrote the file), shows them in busy errors, and `holders::claim_with(key, mode, meta)` writes them. Busy errors end with how to share the device through the hub.
- Forwarding for the ww hub (SRV-29): `Command::Forward { bytes, tag, timeout }` sends a request serialized elsewhere as it is (seq placeholder filled in, no `GlobalFull` retry), `Command::ForwardSink { sink }` hands every answer to it raw to a `ForwardSink` (a trait called on the event loop's receive task; `UnboundedSender<Forwarded>` implements it) as `Forwarded::Reply { tag, event, last }` (stream data under the request's seq included, the last answer frees the seq; `Forwarded::Failed` on a timeout, a refused or lost request), events with seq 0 as `Forwarded::Event` and loopback echoes as `Forwarded::Loopback`, instead of to the client's own streams and observers; `Command::ForwardLoopback` sends a link `Loopback` message as it is, `Command::ForwardFlush` sends what was forwarded so far now (the hub mirrors its clients' flushes; forwarded messages wait for it, 1 ms at most, instead of going out on an idle moment between them). `Forwarded`, `ForwardSink` are in `internal`.
- `DeviceApiInfo::accumulation_time`: the device's accumulation window from its `DeviceInfo` (new pub field).
- `ClientConfig::hub_device(key)` / `hub_device_at(path, key)`: a device through the ww hub's tunnel (SRV-29). `ClientConfig::holder_mode(Mode)`: how the lock files (CLT-38) record this client's USB and RTT devices, `Mode::Hub` for the hub's own connections.
- `decode_introspection(bytes)`: a device's introspection bytes as an `ApiBundleOwned`, with the API snapshots it leaves out put back and generics resolved, as `introspect()` does (the ww hub reads claims from it, SRV-34).
- `ClientConfig::hub_api()` / `hub_api_at(path)`: a client of the ww hub's own API (SRV-28) instead of a device.
- Local socket transport (TR-16), feature `local_socket`: `ClientConfig::ipc_path(path)` connects to a device served by another process on the same PC, over a Unix socket or a Windows named pipe (`ww_local_socket`), user-only on both ends. `local_socket::device::device_medium()` is the matching `ww_device::Server` medium. Selecting IPC without the feature is now an error instead of being skipped.
- `holders` module (CLT-38), always on: before claiming a USB interface or RTT probe, an exclusive
  `std::fs::File::try_lock` is taken on `<runtime dir>/ww/holders/<key>.lock` (`key_for`, `claim`, `holder_of`,
  `list`), content written in extensible `key = value` lines (pid, program, exe, `mode` direct/hub, since, ww
  version). A crash releases the OS lock immediately, independent of the file's (possibly stale) content, which
  `holder_of`/`list` never trust on its own. `usb::connect` and `rtt::event_loop` claim before opening the device
  and name the holder in their busy errors; without a lock file, Linux falls back to scanning `/proc/*/fd` for the
  device node (`holder_of_usb_device`).
- `ClientConfig::client_meta(ClientMeta)` / `no_client_meta()` and the `client_meta` module (CLT-41): who this client is, sent to the device in `LinkSetup`. `ClientMeta` has builders for program, app version, label, kind (`ClientKind`), purpose, user, host and pid; whatever is not set is found by the library (executable name, pid, `$USER`, host name, kind `Cli` when the parent process is a shell, else `Unknown`; `/proc` on Linux, `libc` on macOS, `windows-sys` on Windows). One random instance id per `ClientMeta`, shared by every connection made from clones of the config. A device with a small receive buffer gets a shortened form or none; the connection never fails because of it. `Command::Connect` has the new field `client_meta`.

- `PreparedCall::send()` → `PendingCall`, `reply()` later: requests leave in the order they are sent whatever order
  their replies are awaited in, for calls kept in flight (pipelined `ww_dfu::Stage` steps, CLT-33).
- Per-medium framing (TR-15): USB packets without the split-message CRC-16 and without a prefix by default; the USB transport reads the framing from the device's interface protocol and uses `ww_link::UsbSeqFraming` (a frame counter, as a diagnostic) when the device says so. UDP datagrams start with a `SeqPrefix` frame counter and carry a CRC-32 on every message (`ww_link::UdpFraming`): duplicated, late and previous-session datagrams are dropped, lost ones counted. Wire change, see `ww_link`. `in_process::usb_device_framed::<F>()` simulates a USB device with framing `F`.
- `ww stats` / the tap (TR-15): the reassembly walker reads frame prefixes (`FrameOutcome::prefix`, `prefix_report`) and checks them as `FramedRx` does; `DirStats::prefix` (`PrefixCounts`: lost, duplicate, late, previous session, too short, resynced, new sessions) and `prefix_bytes` show in the report; UDP's CRC-32 and the prefix are decoded.
- `follow::stm32_dfu_update_with` takes its opener as `AsyncFn` and retries it for the new `Timeouts::open` (3 s); `Timeouts` has the new field (CLT-34).
- `follow::stm32_dfu_update()` (feature `stm32_dfu`) and `stm32_dfu_update_with()`: after a firmware rebooted into the STM32 system bootloader (ROM DFU), wait for it under the same serial number, erase, write and verify an image over DfuSe, jump to flash and wait for the firmware to come back (CLT-34). `Follow::back_in_stm32_dfu`, `Found::Stm32Dfu`, `follow::is_stm32_dfu`, `follow::stm32_serial`, `FollowError::Stm32Dfu`; the `ww_stm32_dfu` re-export with the feature.
- Compressed counted streams (API-55): the host offers LZ4 when it opens or resumes a counted stream and decodes the events before they are queued (typed, dynamic and Python clients alike, with `ww_lz4::Decoder`); events after a link loss that can't be decoded until the device's next codec restart come out as one `Gap { cause: Link }` and count in `StreamStats::lost_codec`; `StreamStats::codec_bytes_in` / `codec_bytes_out` say what compression saved. `ClientConfig::stream_compression(false)`, `Commander::set_stream_compression` and `PreparedStreamOpen::compression(false)` turn the offer off.
- `evolution::compare` (`ww api check`) reports a trait's `ww_global` id changed or gone as breaking, an id assigned as compatible; `evolution::diff` (`ww api diff`) lists it as a `ww_global id` change (API-20, EVO-5).
- The tap decoder and `envelope::explain_request` name a `PathKind::GlobalCompact` path through the `ww_global` registry and decode the call from the trait's snapshot like a `GlobalFull` one (API-20, P2401#20oct-1439); `envelope::compact_trait_name` gives the crate version and trait name of a compact id, or why it names none (unknown id, a crate, retired).
- Global trait calls go out with the trait's `ww_global` id (`PathKind::GlobalCompact`, API-20) whenever the registry has one (every `ww_stdlib` trait), also when asked for with `GlobalFull`: 2 bytes and up instead of the crate name. A device built before the trait had an id answers `TraitNotFound`; the event loop then sends the same request again with `GlobalFull`, and sends that trait version's later requests that way for the rest of the connection. The internal, always empty `gid_map` is gone.
- The tap decoder and `envelope` (TOOL-9, TOOL-14): `explain_request` / `explain_event` take the device API as an `Option` and explain requests through `GlobalFull` paths from the snapshots (`envelope::global_trait`), `explain_reply` (an `Observe`'s answer is `ObserveGranted`, not a property), `PropertyUpdate` data read by the level's `+observe` properties (`DynResource::observe_schema`), `Envelope::types` (the types of the payload values, to show units) and `Envelope::resource`. `Decoder::add_bundle` and describe blocks take a device's bundle as it was sent, snapshot-known traits are put back.

### 🐛 Fixes

- Every request waited ~1 ms on the host before it left (CLT-37): the device's accumulation window (250 µs on USB HS) was slept out with a tokio timer, which counts whole milliseconds. A window under 1 ms now ends as soon as the tx task has nothing else waiting. On a B135 over USB HS: a call 1.3 ms → 0.17 ms median.

### ⚠️ Breaking

- Link setup token (TR-15): every connection sends a random non-zero token in `GetDeviceInfo` and `LinkSetup` and drops every message until the `DeviceInfo` carrying it (and a `LinkReady` with another one), so an answer still in the device's buffers from a previous session (a `DeviceInfo`, the rest of a split message) is never taken for the new session; tested end to end (`previous_session_leftovers_are_dropped`). A device without the token, or with an incompatible `dev_link_version`, fails the connection with the new `Error::IncompatibleLinkVersion { device, host }` instead of timing out.
- `StreamStats` has `lost_codec`, `codec_bytes_in` and `codec_bytes_out` (API-55): code building it by hand adds them. `RequestKind::StreamOpen` carries `codecs` (a wire change, see `ww_client_server`).
- `Sink` of a credit sink runs on `ww_stream::sink::SinkTx` (API-34): it opens with `StreamOpen` and a session it names (random), waits for the answer, writes `SinkWrite` / `SinkWriteUnits`, takes `Ack` / `Resend` and opens its session again when the device answers `Close`; it needs a device with the same change. Positions count units: `credit_progress()` returns units (one per value, the bytes of a byte sink's payload), no longer bytes. A device that lost the session (a reboot) fails the call with the new `StreamError::SinkSessionLost { device_at, acked }` (`SinkAhead` for a device ahead of what was sent). `client::credit` (`CreditTx`) is gone, `CreditConfig` is in `client::sink` (still re-exported at the root).

- `Command::Connect` has a `tap: Option<ww_tap::Recorder>` field (None: the default recorder); code building it by hand adds `tap: None`. `TraceEvent` / `Command::RegisterTracer` work as before, now an adapter over the tap's message and link event records, so events arrive batched (at most the recorder's flush time, 10 ms, later).
* The compatibility check no longer knows `Delta` / `DeltaOfDelta` / `XorFloat` (removed from ww_self); a
  `Series<T>`'s codecs are picked per column when writing and are not part of its type, only `T` is compared.
- One rule for every device time the client gives out (P2401#17oct-2214b): `StreamEvent::Item::t` of every
  positioned stream (heads too, not only batch items), `BatchItem::t` and `Observed::t` / `Sample::t` come unwrapped
  whatever prefix and storage the device sends (`U32` µs, `U16` ms, ..; it may change them on the way): a `U64` in
  the same prefix that counts on across wraps, through `ww_si::Unwrapper` without an anchor. Heads of plain counted
  and reliable streams came as the device sent them before, and only `U32` µs was unwrapped elsewhere. Each stream
  handle (each half of a split) and each observe level counts on its own; the count starts over after a disconnect
  and at a new clock epoch in a head's `ext`. `U64` and float times stay as they are.
- **Breaking:** the `rtt` feature's `probe-rs` dependency no longer enables `cmsisdap_v1` (P2401): CMSIS-DAP v1
  (HID-based) debug probes are no longer supported over RTT, only v2 (bulk, no `hidapi`/libusb HID binding pulled
  in). CMSIS-DAP v2 and other probe-rs-supported probes (ST-Link, J-Link, ...) are unaffected.

### 🐛 Fixes

- `disconnect().keep_streams()` could end the kept streams and sinks (`StreamError::Closed`) when the device closed the medium right after the `Disconnect`: the rx half of the event loop took the transport error before tx's word to keep the streams. It now takes in everything tx said first, as it does for a received message.

### 🚀 Features

- The evolution checker (`ww api check`) classifies `#[optional]` changes (API-16, API-54): making an item or `ww_impl!` optional, or putting it into a group, is compatible; making it required again or moving it to another group (or out of its group) is breaking, since servers leaving it out stop compiling. See `docs/evolution/rules.md#optional-items`.
- `Commander::reconnect()` (EX-10, first part): connects again after `disconnect().keep_streams()` through the transport and device selection the Commander was connected with, on the same command channel, then `resume_streams()` (returns its count): the handles of streams, sinks and observers made from the Commander go on. Works for every transport (USB, in-process, RTT, WebSocket, UDP) of a client made by `connect()`; an error when it is still connected, the device isn't there (it can be tried again), or its API differs from before. `connect_keep_trying` and the UI feedback are still to come.
- Credit sinks resume (API-34): `Sink::cursor()` (the session and the units the device accepted, to save with the program's own progress), `Sink::resume(cursor)` (`_blocking`) continues a session after a restart of the host program and returns where the device is, at or after the cursor's position; `DynSink::cursor` / `resume` / `open_session`. Sinks live on through `disconnect().keep_streams()`: `Commander::resume_streams()` registers them on the new connection and opens a credit sink's session again from where the device is, the handle sends what wasn't acknowledged and goes on (nothing repeated, nothing lost); a call while the link is down waits for the resume. A device that lost the session meanwhile (a reboot) is reported by the sink's next call as `StreamError::SinkSessionLost`, `resume_streams()` doesn't count it. `StreamError::NotCreditSink` for a resume of a plain sink.
- Credit sinks (API-34): `Sink::send_units(value, units)` (`_blocking`, `try_send_units`) for a value of other than one unit, `Sink::open_session(session)` (`_blocking`) opens a session user code names (the device continues where it is in that session); `StreamError::BadUnits` for 0 or more than `MAX_WINDOW` units. `DynSink::send_units` (`_blocking`); `DynSink::send` counts one unit per typed value and the bytes of a byte sink's payload, as the typed client does.

- `Error::RemoteError` prints the device's error with its site (SRV-22): `Remote device returned Unimplemented at item site 2 (handler)` instead of the `Debug` of `ErrorOwned`.
- Link tap on the host (TR-14): every client event loop records its link into a `ww_tap::Recorder`, the process-wide `tap::default_recorder()` unless `ClientConfig::tap(recorder)` names one. Transports give the `Session` (medium, framer, frame size, peer) and their frames (USB packets, UDP datagrams, RTT chunks, simulated USB packets), the event loop the messages both ways and the link events (`Connected`, `LinkReady` with the device's `DeviceInfo`, `Disconnected`, `Error`); nothing is copied while nobody subscribes. `wire_weaver_client::tap` re-exports what a consumer needs, `ww_tap` and `ww_tape` are re-exported.
- Tap decoder (TR-14): `tap::Decoder` turns `ww_tape` blocks, live from a `Recorder` or read back from a capture, into `tap::Decoded` frames, messages and link events: every frame walked into its pieces (`ww_framer::pieces`) and reassembled with the framer's rules, so a message knows the frame bytes it was built from (`MessageInfo::frames`) and a frame its pieces and messages (`FrameInfo::outcome`); every reassembled message checked against the transport's record (`Check::Mismatch` is a framer bug); data messages read as requests and events (`ApiInfo`, explained with `envelope::explain_request` / `explain_event` when the device's bundle is known from the capture or the local cache), replies paired with their request by seq, so a return value is explained by its method; `tap::Stats` per link and direction (frames, bytes, fill, overhead, messages per frame, split messages, dropped pieces and partial messages, frame gaps, mismatches, message-to-frame latency, link message kinds, requests with reply times, events), `Stats::report`. Stream media (RTT) are reassembled across chunks. `Decoder::link_api(link)` gives a link's device API once found; a bundle that comes after a link's first data message (a describe block added later) is still used for the messages after it.
- Batched observe in the typed client (API-39): `observe_<name>(mode)` of a `+observe+batched` property gives one
  `Observed` per sample, each with its own device time (unwrapped by the level's device time, the same for every
  observer), whether the device sends one value or a batch; `Commander::prepare_observe_samples`,
  `observe::Extract`, `observe::BatchedOwned` re-exported. The samples of an update share its `sample_times`
  (API-49): a level unwraps them like `t`, a level observer gets them as `Observed::sample_times` (pairs with
  `BatchedOwned::into_samples`), `LevelUpdate::update_latest` takes them; an update whose samples and times don't
  match is malformed (a warning, its samples left out). Compat checks compare the property's `batched` form.

- Batched observe in the dynamic client (API-39): `DynObserver` decodes `Batched` slots of `+batched` properties
  from introspection alone, `DynObserved::samples` (`DynSamples`: name, every sample with its device time from the
  update's `sample_times`, unwrapped by the level as for typed observers; a count mismatch is an error), the newest sample in `DynObserved::values`.
  `DynResource::batched_properties()` lists the level's `+batched` properties.

- `observe::samples(max_age)` / `samples_every(every, max_age)` (API-39): `ObserveMode::Samples` for
  `+observe+batched` properties. Observers of one level merge into `Samples` when any of them asks for it (the
  finest grid, the shortest age any of them waits for).

- Batched streams (API-41): `PositionedStream` and `Stream` of a batched stream give one item per entry of a batch
  (one unit each, its own device time, `u32` µs unwrapped), leaving out the items a resend repeats;
  `recv_batch` / `recv_batch_blocking` give whole batches (`Batch`, `BatchItem`). `PreparedStreamOpen::max_age`
  asks how old a batch may get on the device (asked again on a resume), `PreparedStreamOpen::batched` with
  `unpack_timed` / `unpack_untimed` (used by generated code). `StreamHandle` has an `Item` type.
- Batched streams in the dynamic client (API-41): `DynPositionedStream` and `DynStream` give one `ValueOwned` item
  per entry of a batch, decoded dynamically from `StreamFnRef::batch_item_ty` / `wire_item_ty()` (`ww_self`'s
  `Series`/struct decoder, no generated code involved), with their own device time (`DynQuantity::from_value` reads
  the batch item's `t` field); `recv_batch` / `recv_batch_blocking` on both give whole batches, same shape as the
  typed handles'. `Stream::try_recv_any` (non-blocking variant of `recv_any`), for `DynStream::try_recv` to see a
  reconnect without going through `recv`/`recv_blocking`.

- `quantity` shows `ww_si::AnyQuantity` values (STD-37) like typed quantities: `DynQuantity::from_value` reads the
  unit from the value (`DynUnit::from_si_expr`, a named ww_si unit becomes the same `DynUnit::Known` as the typed
  quantity's), `to_value` writes any quantity whose unit has a dimension into an `AnyQuantity`
  (`DynUnit::to_si_expr`), `is_any_quantity`, and `is_quantity` covers both. New `DynUnit::Named` (a device's own
  unit with its dimension and scale) and `DynUnit::Pow`. `DynUnit` and `DynQuantity` are shown and converted by
  `ww_si::tree`, the code of the typed `Display`, instead of a copy of it; `MAX_UNIT_DEPTH` is `ww_si`'s.

- The `picoboot` feature's `ww_picoboot` now also enables its `elf` feature (CLT-21): `ww_picoboot::Image::from_elf`
  is usable from `follow::rp2_update()` and `ww bootsel flash` without an extra feature flag.

### 🐛 Fixes

- `tap::Decoder` (TR-14): a block's records are decoded in time order (a block stores them track by track, so a reply came before its request and was not paired with it), and the new `DecoderConfig::frames` (default `true`; `false` when the recorder gave messages only) gives a framed link's tx messages out at once instead of `link_wait_ns` (200 ms) later, when no frames are coming to match them. A `DecoderConfig { .. }` written out in full adds `frames`.

- A credit sink's `flush()` after a resume (or a `Resend`) no longer waits a query interval (1 s by default) for the answer: once the items sent again are out, the sink asks where the device is, since the device acknowledges only every threshold bytes. Found on the Donguru (TEST-26): about half of the reconnects with a full window in flight stalled for 1 s.
- A property observer keeps the `missed` of updates without its property (one that carries only the count, API-39)
  and gives it with its next value, instead of dropping it.

- Drop the unused `cfg-if` and `rand` dependencies (P2401, cargo-machete).
- Positioned streams (API-47): an item the device sends again from inside it (a resend, or a resume from a cursor
  confirmed in the middle of a multi-unit item) is no longer dropped as a duplicate or reported as lost:
  `StreamEvent::Item` gives it with `pos` and `units` of the units new to the host, the value is the whole item.
- Counted streams: units dropped because the host queue was full are reported when the stream is disconnected or
  opened again, not left pending; after a device reboot (a new session, positions from 0) the next dropped item
  made the host panic on an underflow (debug builds) or report a gap of about 2⁶⁴ units.

- The compatibility check names an in-line type by its module path, as a skipped one is named: `units::Volt` on one
  side and the same marker left out on the other were reported as `ww_si::units::Volt vs ww_si::Volt` (API-46,
  STD-38).
- Self-referential types (API-44): the compatibility check (`ApiCompat`, the evolution check) remembers the struct and
  enum definitions it compared also when one side writes them out further than the other (a device can), and gives
  up past a depth bound; before, such a pair of types recursed until the stack overflowed.

### 🗑️ Removed

- The iceoryx2 USB packet tracer (`usb::tracing`, feature `usb-tracing`, built but not wired since 558256e1) and the
  `iceoryx2` / `iceoryx2-bb-container` dependencies; the link tap (`ww_tap`) takes its place. The unused `ipc`
  feature is gone too, a local-socket transport brings its own (P2401#14oct-1413). Builds that turned on
  `usb-tracing` or `ipc` drop the feature.

### ⚠️ Breaking

- `Series<T>` (SW-60, CLT-29): dynamic clients read it (`DynResource` values are a `Vec` of items), the
  compatibility check compares a `Series` as a `Vec` of the same item type. A cached bundle in `~/.wire_weaver/` that
  doesn't read in this ww_self format is downloaded and written again. Needs a device built with the same ww_self.
- Introspection data is checked and its generic types instantiated after known definitions are put back
  (`ww_self::generics::prepare`, API-43): malformed data from a device is an error (`Introspect::get` and friends,
  connecting logs it and goes on without the device's API), `IntrospectBundle::from_sent` returns a `Result`. Needs a
  device built with the same ww_self (0.3): the introspection format changed.

- **Breaking:** credit sinks count positions in `u64` and send them modulo 2^28 (API-42): a session has no size
  limit any more (it failed at 4 GiB, "open it again"). `Sink::credit_progress()` and `DynSink::credit_progress()`
  return `Option<(u64, u64)>`. The host never has more than `ww_client_server::credit::MAX_WINDOW` (64 MiB) in
  flight, whatever the device grants. A session over 256 MiB needs a device with the same change.

- `Command::Connect` has a new field `link_setup_timeout_ms: Option<NonZeroU32>` (CLT-30): set `None` to use the
  transport's default. Link setup now gives a device a timeout per transport (USB, UDP, WebSocket 1 s, in-process 250
  ms, RTT 10 s) instead of 250 ms for all, with growing pauses between the retries of `GetDeviceInfo`.

- New `picoboot` feature (off by default, CLT-21): `follow::rp2_update()` and the `ww_picoboot` re-export now need it.
  Previously they came with the `usb` feature (on by default), so every host app that just talks WireWeaver over USB
  also pulled in PICOBOOT (`ww_picoboot`, `nusb`-backed RP2040 / RP2350 boot ROM flashing). `follow::rp2_update_with()`,
  `Follow`, `Bus` and `Found` are unaffected — they don't need `ww_picoboot`'s `usb` feature, which is what
  `tests/follow_reboot` relies on to test CLT-21 against a simulated device without it.
- `ww_picoboot` moved out of this workspace into its own repo, [picoboot_host](https://github.com/vhrdtech/picoboot_host);
  the dependency is unchanged on this side (still `ww_picoboot` in source), only where it comes from.
- `Stream::open()` / `open_blocking()` are gone: a stream is opened by its generated call (API-31),
  `client.<name>().call().await` returns it. For `stream!` (`()` result) opening returns the stream itself, not
  `((), stream)`. `DynResource::stream()` opens plain streams with `StreamOpen`; `DynStream::open()` is gone. Stream
  functions and counted / reliable streams in the API model are `ApiItemKind::StreamFn`: the compatibility check
  compares their delivery, item type, arguments and result, the API diff prints them as `#[stream(T, kind)] fn`.
- The raw stream event `StreamEvent` (`Data` / `Sideband` / `Connected` / `Disconnected`, from `Sink::recv_any` and
  in `StreamError::UnexpectedEvent`) is now `RawStreamEvent`; `StreamEvent<T>` is the event of a counted or reliable
  stream (CLT-23). Code that named it: rename. It has a new variant, `Dropped { events, bytes }` (also in
  `TypedStreamEvent`): exhaustive matches need an arm for it.
- Per-stream queues are bounded (CLT-23): a plain stream (or a property observer) that isn't read for a while
  drops the newest data beyond `ClientConfig::stream_queue_bytes` (4 MiB by default) instead of growing the host's
  memory without limit; `Stream::dropped()` counts them, `recv_any()` reports them once as `Dropped`, `recv()` skips
  that report. Introspection data is never dropped.

- `Error` has a new variant, `Unsupported { what, reason }` (CLT-28), and `internal::Command` one, `ObserveRoute`:
  exhaustive matches need an arm for them.
- `Commander::resume_streams()` also subscribes observed trait levels again (CLT-28) and counts them in its result.

- `TestProgress::SpeedReport` (`internal`) has two more fields, `lost_count` and `data_corrupted_count` (`rx_speed`
  only): code that destructures it adds them or `..`.

### 🚀 Features

- `quantity` module (CLT-31): `ww_si::Quantity<U>` values (ww_si 0.2 and later) recognized from introspection data
  alone. `DynQuantity::from_value(value, ty, bundle)` gives prefix, number and unit, `Display` writes it as `ww_si`
  does (`3300 mV`, `{:#}` `3.3 V`), `to_f64`, `to_si_f64`; `DynUnit` is a named unit of `ww_si::units` (found with
  `units::by_name`: symbol, `dim()`, `factor()`), a `Per` / `Times` tree of them (`N·m/(rad/s)`), or a marker `ww_si`
  doesn't know, by its name and docs (`Unknown`); `unit_of`, `is_quantity`, `DynUnit::from_expr`.
- Generic types (API-43): the dynamic client reads and writes instances of generic types as concrete types;
  compatibility checks compare instances of definitions neither side knows by definition and arguments; the
  evolution check (`ww api check`) compares parameters by position and kind (renaming one is no change). `compare`
  ignores the ww_self version a snapshot was saved with.

- `ClientConfig::link_setup_timeout(Duration)` (CLT-30): how long to wait for a device to answer during link setup,
  overriding the transport's default. Connecting over RTT through a J-Link failed most of the time with the old 250 ms.

- `Observed::t` and `Sample::t` of observers: device microseconds sent as a wrapping `u32` (`Second { Micro, U32 }`,
  what `ww_stream::observe` sends) come unwrapped, as a `u64` that counts on across the 71.6 minute wraps, per
  level and connection.
- Observing properties (CLT-28), module `observe`: `Commander::prepare_observe(path, cfg, extract)` (what the
  generated `observe(cfg)` and `observe_<name>(mode)` call) gives a `PreparedObserve` (`.budget(..)`,
  `.queue_bytes(..)`, `.subscribe()` / `.subscribe_blocking()`) and an `Observer<U, T>`: `recv()`, `recv_blocking()`,
  `try_recv()` give `Observed { value, t, missed, received }`, `recv_event()` also `ObserveEvent::Disconnected`,
  `Resumed`, `Dropped`; `granted()` is what the device applies to the level (`Granted { modes, budget, refused }`),
  `configure(..)` / `configure_with_budget(..)` change what this observer asks for, `with_latest(..)` / `latest()`
  read the level's latest-value cache (`Sample { value, received, t }`). `EventKind::PropertyUpdate` is routed by the
  level's path. All observers of a level share one device subscription, `ObserveConfig::merge` of their settings
  (shortest on-change interval and period per property, biggest budget), sent again when one comes, changes or goes,
  `Off` after the last one. Observers outlive `disconnect().keep_streams()`, a link that goes down for good ends
  them. A device whose API model is `ww_client_server` before 0.2 gives `Error::Unsupported`. Helpers `on_change`,
  `periodic`, `on_change_or_beat` from a `Duration`; traits `LevelUpdate` / `LevelConfig` implemented by generated
  code.
- `DynResource::observe()` / `observe_blocking()` (TOOL-11): observes a trait level's `+observe` properties
  without generated code, by resource index (`observable_properties()` lists them with their type), reusing the
  CLT-28 event-loop routing and subscription sharing through a schema-less `DynUpdate`/`DynConfig` pair instead of
  a generated `<Trait>Update`/`<Trait>Observe`. `DynObserver` (`recv()`, `recv_blocking()`, `try_recv()`,
  `granted()`, `configure()` / `configure_blocking()`, `dropped()`) decodes each `PropertyUpdate` into its present
  properties, by name, from the level's introspection alone (`ww_self::ValueOwned::des_shrink_wrap_field_dyn`
  through the new `OptionAnyVecIter::read_slot_with`), the same bytes a generated client's sparse `<Trait>Update`
  would decode.
- `internal::TraceEvent` is exported, what a tracer registered with `Command::RegisterTracer` receives.
- Generalized multi-request client API (CLT-15): `MultiRead`/`MultiCall`/`MultiWrite` (`client::multi`) are now
  implemented for tuples of 2 to 8 `PreparedRead`/`PreparedCall`/`PreparedWrite` at any depth (previously
  `MultiRead` only, 2-tuple, root level), e.g. `(client.read_x(), client.pin(3).read_mode()).multi_read().await`.
  `client::multi::{read_array, call_array, write_array}` (+ `_blocking`) do the same across an array's indices,
  taking a `MultiIndexOwned` selector (`Range`/`List`/`Mask32`, sent as given) and the existing generated
  per-index accessor as a closure, e.g. `read_array(MultiIndexOwned::Range(UNib32(0)..UNib32(4)), |i|
  client.pin(i).read_mode())`. `MultiArgs::Same`/`Different` is picked automatically by comparing each item's
  serialized bytes. `DynResource` (dynamic client) gets the same through `multi_read`/`multi_call`/`multi_write`.

- `Command::LoopbackTest` works again (TOOL-10): it answered with `FatalError` since the sans-IO event loop. The test
  is a state machine on the tx core (`event_loop/core/loopback.rs`), driven by time and by the `Loopback` echoes rx
  forwards: round trips (`repeat = 1`, one at a time, lost after 500 ms without an echo, corrupted when the
  self-checking data doesn't match), host → device speed (`repeat = 0`, with an echoed marker every 32 messages and
  at most 2 in flight, so a slow device holds the host back on any transport), device → host speed (`repeat = 64`
  requests, at most 2 in flight). Each stage runs `test_duration`; `packet_size` is the link message size, capped to
  what both sides accept, which is also the default. Other requests wait until the test is over; the test fails
  after 3 reply timeouts in a row or when the link goes down, and `progress_tx` is dropped when it ends.
  Its tunables (echo timeout, marker interval and markers in flight, batch size and batches in flight, timeouts in a
  row before giving up) are `Command::LoopbackTest::params`, a `LoopbackParams` (in `internal`) whose `Default`
  has the values above. Before a timer, the tx task now feeds what rx sent meanwhile: a half-duplex device that writes many
  replies blocks host writes for longer than the echo timeout, and the test stalled with 8 batches in flight on a
  full-speed board. A stall says what was in flight and how much was received.

- `const` items are read once per connection (API-36): `Commander::prepare_const_read()` (used by generated clients)
  and `DynResource::read()` / `read_blocking()` on a const keep the value and answer later reads, async, blocking
  and promises, without a request. The cache is shared by a `Commander`'s clones, dropped by `disconnect()` and
  `Commander::clear_const_cache()`; errors are not kept.
- Counted and reliable streams on the host (CLT-23, API-30), see `docs/api/stream_reliability.md`:
  `Commander::prepare_stream_open::<R, S>(path, delivery, args)` opens any stream by calling its stream function
  (`RequestKind::StreamOpen`), `prepare_stream_resume(path, delivery, cursor)` resumes one; `PreparedStreamOpen`
  (`call()` / `blocking_call()`, `with_timeout`, `ack_mode`, `window`, `queue_bytes`) returns the function's result
  with the handle (`StreamOpenResult`: the handle alone, or `Result<(A, S), E>`). `PositionedStream<T>`: `recv_event()` with
  `StreamEvent::Item { pos, units, t, run, value, .. }` / `Gap { at, units, cause, run }` / `Resumed` /
  `BackfillDone` / `Disconnected` / `Closed`, `recv()` for values only, blocking and `try_` variants, `split()` into
  the live and the backfill run, `cursor()`, `ack(pos)` (`AckMode::Explicit`), `stats()` (`StreamStats`), `close()`.
  The event loop runs each stream's `ww_stream::rx::Receiver`: positions, gaps by cause, duplicates, `Ack` and
  `ResendFrom` for reliable streams, the device's `Query`. `ClientConfig::stream_queue_bytes` bounds what a stream
  queues (`DEFAULT_STREAM_QUEUE_BYTES`, 4 MiB). After `disconnect().keep_streams()` the handles of counted and
  reliable streams wait instead of ending; `Commander::resume_streams()` (and `_blocking`) resumes every one of them
  from its cursor on the next connection (the reconnect itself is EX-10), the handle sees `Resumed` and goes on.
- Dynamic client: `DynResource::positioned_stream(cmd, args, ack_mode, resume)` (and `_blocking`) calls a counted
  or reliable stream function with its arguments (or resumes it) and gives `DynStreamOpen::Open { result, stream }`
  (a `DynPositionedStream`, `StreamEvent<ValueOwned>`) or `Rejected(err)`; `DynResource::delivery()`, `stream_fn()`;
  `seconds(&Second)` turns a device time into seconds.

- `Delta`, `DeltaOfDelta` and `XorFloat` in APIs (SW-50): the dynamic client reads and writes them as lists of
  numbers; the evolution checker (`ww api check`) and the connect-time check report another codec or element type,
  or a plain `Vec` instead, as breaking.
- Counted and reliable streams in introspection data (API-30): the compatibility check and API diffs know
  `ApiItemKind::PositionedStream` (a delivery kind that differs between client and device is incompatible), the
  dynamic client lists them as streams.
  dynamic client lists them as streams. Their events (`EventKind::StreamItem`) are dropped until CLT-23.
- Per-method error lists (API-32): a device returning an error outside a method's `#[errors(..)]` logs a `tracing`
  warning naming the method, the error and the list, in generated clients (`call()`, `blocking_call()`, promises,
  through `PreparedCall::with_error_list(ErrorList { .. })`) and in the dynamic client (and so Python).
  `set_strict_error_lists(true)` panics instead, for tests (`tests_common` turns it on). `ww api check` (`compare`)
  reports a list that changed as compatible, naming the entries added (old clients warn about them) and dropped;
  `diff` lists it as a `#[errors]` change.
- Error lists of stream functions and properties (API-32): `PreparedStreamOpen::with_error_list(ErrorList<E>, resource)`
  (on a stream function returning `Result<A, E>`) and `PreparedWrite::with_error_list(ErrorList<CallError<E>>)` (also
  for `write_promise()`), used by generated clients. The dynamic client (and so Python) checks a stream function's open
  error and a property's read and write errors. `ww api check` reports their changed lists as compatible, as for
  methods.
- `Error::is_unimplemented()`: the device has no implementation of an item without an error type (its handler
  returned `Unimplemented`, or `NotImplementedByDevice`). `CallError::device()` and `CallError::is_unimplemented()`.
  `OutcomeError` / `DecodeReply` tie an error type to how a reply is decoded (implemented for `Error` and
  `CallError`).
- Optional items a device leaves out (API-16, listed in its introspection data) fail locally with
  `Error::NotImplementedByDevice`, in generated clients and in the dynamic client (`DynResource::is_omitted()`); they
  are not reported among the incompatible resources after connecting.
- Follow a device across a reboot into other firmware (CLT-21), `follow` module: `Follow::gone()` / `back()` /
  `back_in_bootsel()` / `back_in_wire_weaver()` wait for the device with the same serial number to go away and come
  back as what is expected, on any `follow::Bus` (`UsbBus` for real devices); `Found` tells a WireWeaver device, an
  RP2040 / RP2350 boot ROM in BOOTSEL mode and anything else apart; a timeout lists what had the serial number then.
  `follow::rp2_update()` (`rp2_update_with()` on any bus and PICOBOOT IO) is the second half of an update in the
  RP2350 boot ROM: after `ww_dfu::BootControl::reboot(Updater)` it finds the BOOTSEL device by serial number, writes
  and verifies the image over PICOBOOT (`ww_picoboot`, re-exported), reboots it as `picotool load -x` does, and
  returns the firmware as it came back. `follow::rp2350_serial()` is the serial number a firmware reports so it can
  be followed (the chip id as the boot ROM shows it).
- Global trait calls on any device (CLT-10): `ClientConfig::traits_only()` connects without the user API check and
  without downloading the device's introspection, `DynClient::traits_only(|c| ..)` is a shortcut, and
  `DynClient::global_attachment(gid, trait_name)` gives an `Attachment` whose calls carry a global trait path the device
  resolves itself (API-20). A host built against a new API version can so reboot a device with an old one into its
  updater (`ww_dfu::BootControl`), or read its info traits. Absolute paths on such a connection fail with the new
  `Error::AbsolutePathInTraitsOnly`.
- Simulated USB for in-process devices: `in_process::usb_device(path, max_packet_size)` registers a device that
  speaks USB packets (`UsbSink` / `UsbSource`, wrapped in `ww_device::FramedTx` / `FramedRx` as real endpoints are),
  the host packs messages with the USB framer configuration of `ww_link`, exactly as over nusb. Connect with the same
  `ClientConfig::in_process_path()`. Used by the `ww_dfu` scenario tests.
- `MaxLen` bounds (SW-19) in compatibility checks: on connect a different bound is not an error (a longer value is
  rejected when it is sent); in `ww api check`, lowering or adding a bound is breaking and raising or removing one is
  compatible (`max length raised`). The dynamic client checks bounds of arguments and sink writes.
- `Sink::send_owned()` for sinks of owned types that hold a `Vec` or `String` (`FooOwned`, which implement
  `SerializeShrinkWrapOwned`, not `SerializeShrinkWrap`); `send()` could not send them.
- Credit sinks (`#[credit] sink!(..)`, API-28, `docs/api/streams.md`), credit counted in bytes: `Sink::send()` /
  `send_blocking()` / `send_owned()` wait until the device granted room for all bytes of the value (values are never
  split), fail with the new `StreamError::ItemTooBig` when a value is bigger than the device's receive buffer (the
  first credit after `Open`), `try_send()` / `try_send_bytes()` return `false` instead (for promise
  style code), `flush()` / `flush_blocking()` / `try_flush()` wait until the device accepted everything, lost writes
  are sent again from the device's position (go-back-N), `close()` flushes first. `CreditConfig` (query interval and
  stall timeout) via `Sink::set_credit_config()`, `Sink::credit_progress()` (bytes) for progress bars. `DynSink` speaks it
  too. `Commander::prepare_credit_sink()` / `_blocking()`.
- `Sink::send_blocking()` for typed sinks, `Sink::try_send()`, `send_owned_blocking()`, `try_send_owned()`; values are
  no longer limited to 1024 bytes.
- Compatibility checks (device vs host on connect, and `ww api` evolution checks) treat `sized` and `self_describing`
  as the same layout: a type can move between the two without being reported as breaking (SW-35).
- `Sink::recv_sideband()`, `recv_sideband_blocking()`, `try_recv_sideband()` and `recv_any()` / `recv_any_blocking()`:
  sideband events from the device (replies to `Sink::sideband()` and events it sends on its own) are no longer
  dropped.
- Generated clients have `observe_<property>()` / `observe_<property>_blocking()` for `ro` and `rw` properties: a
  `Stream` of property updates the device sends as stream data on the property's path.
- WebSocket transport (`ws` feature, replaces the outdated `wire_weaver_net_host` crate):
  `ClientConfig::websocket_url("ws://host:port/path")` connects through the regular `PreparedConnection` flow (link
  setup, version check, introspection, timeouts, streams), same as USB or RTT. Each `ww_link` message is one binary
  WebSocket message, `[kind: u8][payload ..]`, without `ww_framer`; messages are buffered until the event loop flushes
  and Nagle is off. Only plain `ws://` for now, no `wss://` and no discovery. Selecting WebSocket without the
  feature fails with an explicit error.
- UDP transport (`udp` feature, replaces the outdated `wire_weaver_udp_link` crate): `ClientConfig::udp_addr("host:port")`
  connects through the regular `PreparedConnection` flow, same as USB or WebSocket. One `ww_framer` frame per
  datagram with `ww_link`'s UDP framer configuration, at most `ww_link::UDP_MAX_DATAGRAM_LEN` (1452 B) sent, any size
  received. Nothing is retransmitted: lost requests time out, lost stream events are gone. If nothing listens on the
  device port, connecting fails right away. No discovery. Selecting UDP without the feature fails with an explicit
  error.
- In-process transport (`in_process` feature): a device running in the same process (simulators, tests) registers
  a path with `in_process::device(path, max_message_len)` and serves `ww_device::Server` with the returned
  `DeviceTx` / `DeviceRx`, the host connects with `ClientConfig::in_process_path(..)` through the regular
  `PreparedConnection` flow (link setup, version check, introspection, timeouts, streams). `in_process::TokioClock`
  is a `ww_device::Clock` for devices on the host. Integration tests under `tests/` now run on it, against the real
  event loop.

- `DynResource`: use any device API known only at runtime (from introspection data or a saved bundle), without
  generated code. Walk it by names and indices (`root.child("periph")?.index(0)?.child("gain")?`), call methods, read
  and write properties with `ww_self::ValueOwned` values, open streams (`DynStream`) and sinks (`DynSink`), read valid
  indices of arrays; async and blocking variants. Bytes on the wire are the same as with a generated client.
  `DynResourceKind` tells what a resource is, with its argument, return and property types. `ww_numeric` is
  re-exported.
- RTT transport (`rtt` feature): `ClientConfig::rtt(target, speed_hz)` connects through a debug probe with probe-rs,
  to a device serving over RTT (`ww_device::rtt`). The probe is selected with VID:PID and serial filters placed
  after `rtt(..)`, filters before it describe the device (e.g., a driver crate's `default_config()`) and are not
  used for the probe; WireWeaver USB is not tried with RTT selected. Probe IO runs on a dedicated thread polling the
  `ww_up` / `ww_down` channels, the framing is the stream mode shared with the device (`RttHead` / `RttChecksum` /
  `RttTail` from `ww_link`). Selecting RTT without the feature fails with an explicit error instead of
  "no devices found".
  `ClientConfig::rtt_elf(path)` / `rtt_control_block_at(address)` skip scanning RAM for the RTT control block
  (~1.5 s for 144 KiB through an ST-LINK, tens of milliseconds with the address known); RAM is scanned anyway if the
  running firmware doesn't match.
- Per-resource API compatibility checks. If the device reports the same API hash as the client was generated with,
  nothing is checked. Otherwise, client and device introspection data are compared for every method, property,
  stream and trait (argument, return, property and stream types, property access, trait origin), following the
  evolution rules: e.g., an `Unsized` struct may gain trailing fields, but the reading side's extra fields need
  `#[default]`. Using a missing or incompatible resource fails locally with `Error::NotImplementedByDevice` or
  `Error::IncompatibleResource`, instead of sending a request the device would misread; all such resources are
  logged once on connect. If the device API is unknown (introspection disabled and not cached), `#[since]` of the
  resource is checked against the device's API version and fails with `Error::OlderProtocol` (was not checked before).
  Only absolute paths are checked, trait-client paths without an attachment base path are not.
  A trait or type whose definition is skipped on one side is checked by origin and crate version, and by signature
  when the versions are the same, which catches a definition changed without bumping its crate version.

- `evolution` module, the evolution checker: `evolution::compare(old, new)` compares two crate snapshots
  (`ww api save` bundles) and returns a `Report` with the kind of change (`Change::None`, `DocsOnly`, `Compatible` or
  `Breaking`), the reasons, and warnings (additions without `#[since]`). `Report::check_version()` fails if the new
  crate version doesn't bump the required position: breaking changes need the breaking position, any other change,
  doc comments included, the compatible one. Resources are compared both ways with the same rules as the
  per-resource compatibility check; renames and removals are breaking too, as they break Rust code.
- `evolution::diff(old, new)` lists every difference between two crate snapshots as `evolution::Difference`s (added,
  removed, changed, or doc comments changed), doc comments included, without classifying them.
  `Report::minimal_version()` gives the smallest version the new one has to be.
- Per-resource compatibility check: types may gain new fields in between the old ones, in their unused padding bits,
  if no old field moves and the new ones read all zero bits as a valid value. `Unsized` types always start at a byte
  boundary, `final_structure`, `self_describing` and `sized` ones are checked for every bit offset they can start at.
  Before, such fields were only compatible at the end of an `Unsized` type (with `#[default]`), and other types
  were incompatible whenever the number of fields differed.

- `snapshots` (re-export of `wire_weaver_snapshots`): API snapshots of `ww_global` and `ww_stdlib` crates, looked up
  by crate name and version with `snapshots::get()` or `snapshots::all()`. The per-resource compatibility check uses
  them to calculate the signature of an in-line trait or type that refers to skipped ones, which was not checked
  before.
- Introspection data downloaded from a device or loaded from `~/.wire_weaver/` has the traits and types the device
  left out (because they are known from snapshots) put back from snapshots, if their signatures match, so
  `Introspect::get()` and friends return the full API. Ones that are not known are logged and left skipped.
  The same is done for the client's own API embedded by codegen.

- `DeviceInfo` is filled from the USB API id interface string (falling back to the product string): API name,
  version, truncated hash and user label are known without opening the device, so `.user_label_eq()` and
  `.implements_api()` filters now work for USB. `DeviceInfo` gains `location` and implements `Display`.
- `list_usb_devices()` lists devices reporting an API id, `list_all_usb_devices()` lists all USB devices, neither
  opens a device.
- `ClientConfig::client_version()` sets the API crate name and version sent to the device during link setup.
- `ClientConfig::serial_contains()` selects a device by a part of its serial number.
- `ClientConfig::matches(&DeviceInfo)` checks a device against the config filters, e.g. to list only the devices a
  connection would consider.
- `DeviceInfo` gains `usb: Option<UsbLocation>` (bus id, port chain, VID and PID).
- `Introspect::download()`/`download_blocking()` (and `get*()` on a cache miss) save the downloaded API bundle to
  `~/.wire_weaver/`, so a device whose API was seen once is not downloaded again on the next connect. The bundle is
  only cached if its hash matches the one the device reported.
- `Introspect::with_timeout()` sets the introspection download timeout, restarted after each received chunk
  (defaults to the Commander's default timeout).
- `Stream::recv_all_bytes_timeout()`/`recv_all_bytes_timeout_blocking()` fail if no event arrives within the timeout.

### 🐛 Fixes

- A downloaded API bundle is cached in `~/.wire_weaver/` only when the device's API crate name is a plain crate name
  (letters, digits, `_`, `-`): the name made the file name as it was, so a device could name a path outside the cache
  (`../x`), and with the rewrite of cache files that don't read back, write there. Other names are logged and not
  cached.

### ⚠️ Breaking

- `DynResourceKind::Method` has an `errors` field (API-32), matches need `..`.
- One `?` per call (CLT-24, `docs/api/methods.md`): a method returning `Result<T, E>` gives `Result<T, CallError<E>>`
  from `call()`, `blocking_call()` and `call_promise()` instead of `Result<Result<T, E>, Error>`. `CallError` is
  `Device(E)` (the method's `Err(e)`), `Unimplemented { resource }` (the handler returned `Unimplemented`, or the
  device's API doesn't have the method: `NotImplementedByDevice`) or `Client(Error)`; `Display` and `source()` are the
  inner error's, `std::error::Error` when `E` is. Replace `??` with `?` and `Ok(Err(e))` with
  `Err(CallError::Device(e))`; a `thiserror` enum takes `#[from] CallError<E>`. Methods without `Result` keep
  `Result<T, Error>`.
- A property with a user error type (`property!(rw mode: u8, ModeError)`) gives `Result<(), CallError<ModeError>>` from
  `write()` / `blocking_write()` and `Promise<(), CallError<ModeError>>` from `write_promise()`, the user error as
  `CallError::Device` instead of `Error::RemoteError` with `ErrorKind::UserBytes`. `Error::RemoteErrorDes` (the user
  error formatted into a string by `write_promise()`) is removed.
- `PreparedCall<T, E = Error>`, `PreparedWrite<E = Error>`, `Promise<T, E = Error>` and `PromiseState<'_, T, E = Error>`
  take the error type (`PreparedWrite`'s parameter was the user error type, `()` without one; now it is `Error` or
  `CallError<UserError>`). `Promise::take()` needs `E: Display`. `Commander::prepare_call()` and `prepare_write()` take
  a `resource: &'static str`, the item's name for `CallError::Unimplemented` (generated code passes `Trait::item`).
- `Sink::open()`, `close()` (and `_blocking`) take `&mut self`, same for `DynSink`: on a credit sink they reset or
  flush its state.
- `DynResourceKind::Stream` has a `flow: Option<FlowControl>` field.
- `ClientConfig::websocket_addr(addr, port, path)` is replaced by `ClientConfig::websocket_url(url)`, which also
  takes host names. The unused `net` feature is removed, use `ws`.
- `ClientConfig::udp_addr(addr, port)` takes a single `"host:port"` string now (host names are resolved when
  connecting), e.g., `udp_addr(format!("{ip}:{port}"))`.

- `PreparedWrite::write_promise` returns `Promise<()>` instead of `Promise<E>`. `PreparedWrite<E>`'s `E` is now the
  property's user error type (`()` if it has none) instead of `Result<(), E>`, generated `write_*` functions changed
  accordingly. Code naming these types has to be updated, e.g., `Promise<Result<(), MyError>>` → `Promise<()>`.

- `SeqTy` is `u32` (was `u16`). Request seq numbers cycle through 1..=127, so they are serialized into 1 byte, bigger
  ones (up to `ClientConfig::max_seq()`, `DEFAULT_MAX_SEQ` = 2 097 151 by default, 3 bytes) are only used while
  all of these are in flight. `Command::Connect` gains `max_seq`. `Command::SendMessage` bytes must start with the
  5-byte seq placeholder of a serialized `ww_client_server::Request` (the unused part of it is dropped before
  sending), also for requests without an answer, which are now sent with seq 0 trimmed to 1 byte. Wire-incompatible
  with devices built against the previous `ww_client_server`.
- `Error` gains `NotImplementedByDevice` and `IncompatibleResource` variants, exhaustive matches need new arms.
- `IntrospectBundle` gains `sent_size`, the size of the introspection data as sent by the device, and
  `sent_api_bundle`, the introspection data as sent, before traits and types known from snapshots were put back into
  `api_bundle`; code constructing it must set both.

- `connect()`/`connect_blocking()` report why they failed instead of a generic "No devices found to connect to":
  - `Error::DeviceNotFound` is now a struct variant with the config's `filters` and the connected WireWeaver
    devices that did not match them (`unmatched`), both printed in the message. Match it with
    `Error::DeviceNotFound { .. }`.
  - New `Error::ConnectFailed { device, reason }` when the selected device fails to open or complete link setup;
    busy and (on Linux) permission denied errors include a hint on the likely cause.
  - New `Error::NoTransportSelected` when the config selects no transport (e.g., `.usb()` is missing).
  - Failing to list USB devices is returned as an error instead of being ignored.
- Device filters of different kinds are now all required instead of any one being enough, e.g.
  `.usb_vid_pid(..).serial_eq(..)` selects only the device with that serial, not every device with that VID:PID.
  Filters of the same kind are still alternatives. Configs relying on the old "any filter" behavior must drop the
  filters that shouldn't restrict the selection.
- `.usb()` without any filters now selects the only connected USB device reporting a WireWeaver API id, instead of
  selecting nothing. Without `.usb_vid_pid()` or `.usb_port_chain()`, devices not reporting an API id are not
  considered.
- `DeviceInfo` has a new public field `usb`, code constructing it must set it.
- `Introspect::download_promise()` is renamed to `Introspect::get_promise()`, since it checks the cache first like
  `get()`. It now resolves to an error if the device has introspection disabled, instead of a deserialization error.

### 🐛 Fixes

- The padding-bit layout check expands nested arrays of a device's types only up to 65 536 leaves, instead of
  without limit.
- The padding-bit layout check (compatibility of a device's types that gained fields) is bounded in its work too, not
  only in array leaves: in-line structs with many fields of other such structs made exponentially many leaves (or
  visits, for empty ones) from a few hundred bytes of introspection data and hung the host; now "types are too large
  to compare their bit layouts". Old fields are looked up by path and overlaps found by binary search, instead of
  quadratic scans over up to 65 536 leaves.

- Dropping a `Commander` clone no longer disconnects the device for every other clone: only the last one to drop does. `ww info` and `ww report` ended with "Called a method that required event loop to be running" because the trait clients of `ww_report::Contract` disconnected on drop and `ww` then disconnected again.

- A device whose skipped trait or type has another signature than the host's snapshot (built from another revision
  of an unpublished crate, e.g. `ww_si::Volt` on the Donguru M0+ firmware, P2401) is no longer refused as "has a
  different definition on each side" (EVO-9): its definition is looked up by signature in the definition cache
  `~/.wire_weaver/definitions/`, put back with a warning and compared by the evolution rules. Only when it is not found
  does a resource using it fail, with both signatures, the likely cause and how to fix it.
- `multi_read()` returns an error instead of panicking when the device's answer doesn't deserialize, or for
  resources below the root level (not supported yet), and no longer prints the answer to stdout (CLT-15).
- A request sent while every seq number is in flight waits for one instead of failing with "No more request IDs
  available" (CLT-18), and goes out in order once a reply frees one. A fire-and-forget stream sideband sent right
  after a call returned could get lost this way with a small `ClientConfig::max_seq`.
- USB: endpoint addresses and the interface number are no longer hardcoded (`0x01` / `0x81` on interface 0). They are
  read from the active configuration descriptor. The client picks the interface with WireWeaver class/protocol
  (`0xFF` / `0x37`), or for devices not reporting it, the first interface with a bulk or interrupt IN/OUT pair.
  Composite devices where the WireWeaver interface isn't first, or gets other endpoint numbers, now connect.
- Stream and sink constructors (generated `<stream>()` / `<stream>_blocking()`, `observe_<property>()`,
  `Commander::prepare_stream` / `prepare_sink`, `DynStream`, `DynSink`) returned before the event loop started routing
  events to them, so events the device sent right after were sometimes dropped and a following `recv()` waited
  forever. They now return once the stream is registered.
- A stream sideband request (`open()`, `close()`, `sideband()` on `Stream` and `Sink`) kept its seq until the default
  timeout, even when the device replied with a sideband event: the reply was only routed to the stream. Its seq is
  now released when the reply arrives, so a burst of sideband requests no longer runs out of request IDs.
- `write_promise` never succeeded: the empty reply was deserialized as `Result<(), E>` and failed with
  `OutOfBoundsReadBool`. A user error (`property!(rw name: T, UserError)`) was deserialized as `Result<(), E>` as
  well and failed, it is now formatted with the user error type into `Error::RemoteErrorDes`. `read_promise` no longer
  deserializes a user error as the property type, it returns `Error::RemoteError` with the raw bytes, same as `read()`.

- A late reply to a timed out request is no longer taken for the answer to a new request that got the same seq: the
  seq of a timed out request stays in use until the late reply arrives (which is then dropped) or until 10 s after the
  request was sent (`LATE_REPLY_WINDOW`), its own timeout still fails the call on time.
- Evolution diff and padding reuse checks tell a relocated `#[flag]` (`TypeOwned::Flag`) from the `Option` or
  `Result` field with the same name, a flag is a 1-bit field in the layout.
- Connect failing because the transport itself could not be opened (e.g., USB interface busy) could report a dropped
  channel instead of the actual reason.
- `connect()` returns `Error::AmbiguousDeviceChoice` listing the matched devices one per line, instead of printing
  them to stdout in Debug format and returning a generic "Ambiguous device choice" error.
- Introspection cache lookup falls back to the bundle without doc strings when the one with doc strings is not
  cached, and matches exact file names instead of any name containing the hash.
- A missing `~/.wire_weaver/` directory or an empty device API hash is a silent cache miss instead of a warning on
  every connect.
- Introspection download no longer hangs `connect()` forever if the device never answers, it times out instead.
- Failing to download introspection data during `connect()` is logged instead of silently ignored.
- `PreparedDisconnect::asynch()`/`blocking()`/`keep_streams()`/`keep_streams_blocking()` return only after the
  transport is closed and the USB interface is released. Connecting to the same device right after a disconnect
  used to fail with "interface is busy (errno 16)". Queued outgoing USB packets (including the final Disconnect) get
  up to 100 ms to go out before they are cancelled.

- Connect intermittently failed (about half of the attempts on real hardware): when the transport's rx half and
  `TransportUp` arrived at the same time, the rx task could drop the rx half, closing the USB IN endpoint. The device
  reply was then never read, and a stale `DeviceInfo` was left for the next session.
- First request sent right after `connect()` returned could be rejected with "ignoring SendMessage while
  disconnected", because rx unblocks the caller before tx sees `LinkReady`. Requests arriving during link setup are
  now held and sent once the link is up, or failed with `Disconnected` if setup fails.
- Generated clients never sent their API version (empty `crate_id` in `LinkSetup`), so they were treated as dynamic
  clients and version compatibility was not checked on either side. It is now set automatically.
