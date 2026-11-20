# ww_dns_sd changelog

## Unreleased

### 📚 Documentation

- A fixed broken doc link in the crate docs (`AvahiResponder`, behind the `avahi` feature).

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.
- `AvahiResponder` runs on zbus 6: `with_connection` takes a `zbus::Connection`, `Error::Avahi` holds a zbus 6 `zbus::Error`.

### 🚀 Features

- New crate (TR-32): `ServiceRecord`, `Txt`, `Advertiser`, `Responder`, `start_responder`, `interface_addresses` announce WebSocket servers as `_wireweaver._tcp`.
- `MdnsResponder` (feature `mdns`, default): in-process mDNS responder; `AvahiResponder` (feature `avahi`): Avahi over D-Bus (TR-32).
