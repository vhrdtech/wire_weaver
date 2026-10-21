# Changelog

## Unreleased

### 🚀 Features

- New crate (SRV-28): the ww hub process. `serve(HubConfig)`: one hub per user (`hub.lock`), the local socket in the
  runtime dir with the `WWHB` preamble picking the hub API or a device, connected programs tracked, exit after
  `idle_exit` without clients or devices (`Hub::keep` holds it up), `shutdown`.
