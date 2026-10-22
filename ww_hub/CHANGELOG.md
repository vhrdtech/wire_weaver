# Changelog

## Unreleased

### ⚠️ Breaking

- `HubConfig` has the new field `adopt_attach` (CLT-44, `ADOPT_ATTACH_ENV`, 5 s by default): struct literals need it.
- `HubConfig` has the new field `version` (the version the hub answers with and compares in a handover; `HUB_VERSION` from `from_env()`), `Exit` the variant `Handover`. Struct literals and exhaustive matches need them.

### 🚀 Features

- Live link handover (CLT-44): `ww_hub_api::Adopt` is served. `prepare(snapshot)` checks a direct holder's link
  snapshot (the same `wire_weaver_client` minor, a medium that fits the device key, the device not open in the hub
  nor lent) and keeps the device's slot, so nobody opens it meanwhile; it goes with the API connection. `commit(id)`
  opens the device going on with the link (no link setup), the holder's tunnel joined with its requests in flight,
  streams and observed levels, and waits `HubConfig::adopt_attach` for that tunnel (preamble `adopt` id) before
  taking the holder as gone. A tunnel sends its queued events only once its client's link is up.
- Device key `usbsim:<socket path>`: a simulated USB device on a local socket (`wire_weaver_client::usb_sim`).
- The hub's own connections to its devices don't steer by themselves (`ClientConfig::clock_sync(false)`, CLT-46):
  the hub steers them with its sync (SRV-31). Clients reaching a device through the hub leave the steering to it
  (`Phase::ThroughHub`). The hub uses `wire_weaver_client::clock_sync` (`serves_host_sync`, `rate_key` moved there)
  instead of the `ww_clock_sync` crate; `HubConfig::sync_options` is the same type.
- Device key `ipc:<socket path>`: a device another process serves on a local socket (`wire_weaver_client::local_socket::device`: a simulator, host-emulated firmware), opened with `ClientConfig::ipc_path`; it outlives the hub.
- Handover to a newer hub (SRV-30): `Hub::handover(socket)` is served (was `Unimplemented`): the hub checks the version the hub on `socket` answers its preamble with, refuses one that isn't strictly newer (`HandoverError::NotNewer`; `Unreachable` when nothing answers), then stops accepting (its socket goes at once), sends `Event::Handover` to the registry's listeners, closes every tunnel and API connection with `DisconnectReason::HubHandover`, closes its devices and exits with `Exit::Handover`, releasing `hub.lock`. `Bound::take_over(config)` is the other side: listens on a socket of its own, asks the running hub to hand over, takes `hub.lock` and moves its socket into place (rename on Unix, the pipe created anew on Windows), `TakeOverError` when it can't; `serve()` does it when started with `HANDOVER_ENV`. An exiting hub waits up to 500 ms for its connections to send what they still have. `strictly_newer(a, b)`.
- Version window (SRV-30): a connection whose preamble names `ww_link` or `ww_client_server` versions the hub's build doesn't serve (compatible by SemVer with its own, or in the window it keeps after a breaking release) is answered `Status::Versions` (4) and closed; a client that doesn't say (preamble version 1) is served as before.
- Device-wide exclusive and lending (SRV-32): `Claims::lend` served, the device closed with its tunnels told why (`Disconnect(Other(DISCONNECT_LENT))`) and only then answered, new tunnels refused with `Status::Lent`, `take` and others' claims refused naming the lend, the device opened again on return when the hub held it (kept if it was taken). Claims with a ttl now expire as the hub runs, not only when someone asks. A root claim (or a lend) pauses clock steering of the device (`steering_paused`, used by SRV-31's steering).
- Clock sync in the hub (SRV-31): every device the hub opens that serves `ww_timebase::Clock` with `host_sync` (found in its introspection data at any path) is steered onto this PC's UTC by a `ww_clock_sync::ClockSync` on the hub's own connection, with the hub as the steerer (`ww hub`, its pid, a random id) and the rate cache in the state dir; a steered device stays open for `HubConfig::sync_linger` (30 min, `WW_HUB_SYNC_LINGER` seconds or `forever`) after its last client left instead of `linger`. New `HubConfig` fields `sync` (`WW_HUB_SYNC=off`), `sync_linger`, `sync_options`, `state_dir`. Serves `ww_hub_api::ClockSync`: `devices`, `set_enabled`, `linger`, `stats`. While a program holds the whole device (a root claim or a lend, SRV-32) the device is still read but not steered (`SyncPhase::Paused`); claims coming and going wake the steering at once.
- `main_if_hub()` answers `--hub-version` (prints `HUB_VERSION`, exits) and registers the executable with `wire_weaver_client::hub::register_self_hosting`, so a client can start the hub from the app itself (CLT-39). `SERVE_ENV` is `wire_weaver_client::hub::SERVE_ENV`; `client::HubClient` is `wire_weaver_client::hub::HubClient` re-exported.

- `Hub::status` served (TOOL-17): who started the hub (`WW_HUB_STARTED_BY`, else the parent process's name on Linux), idle exit, seconds until it exits or of a `keep`, open connections. `Registry::devices` fills location, label and API hash.
- New crate (SRV-28): the ww hub process. `serve(HubConfig)` / `Bound::bind` + `run`: one hub per user
  (`hub.lock`), the local socket in the runtime dir with the `WWHB` preamble picking the hub API or a device, the
  `ww_hub_api` server per connection (info, keep, shutdown, registry of devices, holders and clients, events), exit
  after `idle_exit` without connections; `main_if_hub()`, `client::HubClient`.
- `run_blocking()` / `run_blocking_with(config)` serve on a one-thread tokio runtime: a message through a tunnel
  passes several tasks, and on one thread each hand-over is a task switch instead of a wake of another thread
  (fewer µs per round trip, measured with `tests/hub/examples/hub_loopback.rs`). The lock files are listed on a
  blocking thread.
- Device tunnels (SRV-29): a connection whose preamble names a device (`usb:<vid>:<pid>:<serial>`, `rtt:<probe serial>:<chip>`, or `inproc:<path>`
  for a device served in the hub's process) gets a link endpoint that answers like the device (its `DeviceInfo`,
  `LinkSetup` checked by `ww_device::DeviceLink`). The hub opens the device on demand with one `wire_weaver_client`
  event loop for all its tunnels (lock file `mode = hub`), forwards requests with seqs of its own and puts each
  client's seq back on the answers, opens a stream on the device for its first client and closes it after the last,
  sends stream data to the clients that opened the stream, merges observe settings, answers introspection from a
  cache, queues at most 1 MiB of stream data per client (dropped and counted beyond), and closes the device
  `HubConfig::linger` (30 s, `WW_HUB_LINGER`) after its last client left. `Registry::take(key)` opens a device and
  keeps it; `Registry::devices` shows the hub's devices as `Open` with their tunnels, events `Opened` / `Closed`.
  The hub reads only seq, path and kind of what it forwards, never a user API.
- Per-resource claims (SRV-34): the `claims` group of the API (`claim`, `release`, `claims`, `Claimed` / `Released`
  events). What can be claimed comes from the device's introspection data (`#[claim]`, `#[shares]`, `#[label]`),
  fetched by the hub itself; while a client instance holds a claim, the hub answers other clients' calls and writes
  at or under its path with `Common(InUse { by })` in the item's error type (an error event without one), without
  reaching the device; reads, streams and observe stay open unless the claim is `observe`. Claims belong to the
  client instance (`ClientMeta` instance id) and go with its last connection or at their ttl.
