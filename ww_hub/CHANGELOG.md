# Changelog

## Unreleased

### 🚀 Features

- Version window (SRV-30): a connection whose preamble names `ww_link` or `ww_client_server` versions the hub's build doesn't serve (compatible by SemVer with its own, or in the window it keeps after a breaking release) is answered `Status::Versions` (4) and closed; a client that doesn't say (preamble version 1) is served as before.
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
