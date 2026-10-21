# Changelog

## Unreleased

### 🚀 Features

- `main_if_hub()` answers `--hub-version` (prints `HUB_VERSION`, exits) and registers the executable with `wire_weaver_client::hub::register_self_hosting`, so a client can start the hub from the app itself (CLT-39). `SERVE_ENV` is `wire_weaver_client::hub::SERVE_ENV`; `client::HubClient` is `wire_weaver_client::hub::HubClient` re-exported.

- `Hub::status` served (TOOL-17): who started the hub (`WW_HUB_STARTED_BY`, else the parent process's name on Linux), idle exit, seconds until it exits or of a `keep`, open connections. `Registry::devices` fills location, label and API hash.
- New crate (SRV-28): the ww hub process. `serve(HubConfig)` / `Bound::bind` + `run`: one hub per user
  (`hub.lock`), the local socket in the runtime dir with the `WWHB` preamble picking the hub API or a device, the
  `ww_hub_api` server per connection (info, keep, shutdown, registry of devices, holders and clients, events), exit
  after `idle_exit` without connections; `main_if_hub()`, `client::HubClient`.
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
