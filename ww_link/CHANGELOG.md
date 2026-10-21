# ww_link changelog

## Unreleased

### ⚠️ Breaking

- Link setup token (TR-15): `Message::GetDeviceInfo { token }` and `Message::LinkReady { token }` carry a `u64` (LE payload, none from older peers = 0), `DeviceInfo::token` and `LinkSetup::token` are new fields (`#[default = 0]`, so older peers still decode). The host picks a random non-zero token per connection, the device echoes it and the host drops everything before the reply carrying it, so that an answer left from a previous session is never taken for the new one. Wire change: hosts and devices without the token can't connect to ones with it (they get `Disconnect(IncompatibleVersion)` / `IncompatibleLinkVersion`). Match `Message::GetDeviceInfo { .. }` / `Message::LinkReady { .. }` instead of the unit variants.

### 🚀 Features

- Framer configuration for RTT: `RttHead`, `RttChecksum`, `RttTail` (`U2Head`, no checksum, no tail).
- Framer configuration for UDP: `UdpHead`, `UdpChecksum`, `UdpTail` (same as USB, one frame per datagram) and
  `UDP_MAX_DATAGRAM_LEN` (1452), the largest datagram a host sends.
- First version: sans-IO, `no_std` link layer — message kinds and their payloads carried over `ww_framer` frames.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.
