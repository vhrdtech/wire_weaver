# ww_dns_sd changelog

## Unreleased

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.

### 🚀 Features

- New crate (TR-32): `ServiceRecord`, `Txt`, `Advertiser`, `Responder`, `start_responder`, `interface_addresses` announce WebSocket servers as `_wireweaver._tcp`.
- `MdnsResponder` (feature `mdns`, default): in-process mDNS responder; `AvahiResponder` (feature `avahi`): Avahi over D-Bus (TR-32).
