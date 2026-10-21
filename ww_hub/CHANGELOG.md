# Changelog

## Unreleased

### 🚀 Features

- `main_if_hub()` answers `--hub-version` (prints `HUB_VERSION`, exits) and registers the executable with `wire_weaver_client::hub::register_self_hosting`, so a client can start the hub from the app itself (CLT-39). `SERVE_ENV` is `wire_weaver_client::hub::SERVE_ENV`; `client::HubClient` is `wire_weaver_client::hub::HubClient` re-exported.

- `Hub::status` served (TOOL-17): who started the hub (`WW_HUB_STARTED_BY`, else the parent process's name on Linux), idle exit, seconds until it exits or of a `keep`, open connections. `Registry::devices` fills location, label and API hash.
- New crate (SRV-28): the ww hub process. `serve(HubConfig)` / `Bound::bind` + `run`: one hub per user
  (`hub.lock`), the local socket in the runtime dir with the `WWHB` preamble picking the hub API or a device, the
  `ww_hub_api` server per connection (info, keep, shutdown, registry of devices, holders and clients, events), exit
  after `idle_exit` without connections; `main_if_hub()`, `client::HubClient`.
