# Changelog

## Unreleased

### 🚀 Features

- New crate (SRV-28): the ww hub process. `serve(HubConfig)` / `Bound::bind` + `run`: one hub per user
  (`hub.lock`), the local socket in the runtime dir with the `WWHB` preamble picking the hub API or a device, the
  `ww_hub_api` server per connection (info, keep, shutdown, registry of devices, holders and clients, events), exit
  after `idle_exit` without connections; `main_if_hub()`, `client::HubClient`.
