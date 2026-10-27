# ww_link changelog

## Unreleased

### 🐛 Fixes

- `Kind::Data1` and `Kind::Data2` are documented as reserved for a later use, not free for applications (a device drops them).

### ⚠️ Breaking

- Per-medium framing (TR-15), wire change of USB and UDP: `UsbChecksum` is `NopChecksum` (USB checks, retries and orders every packet; stale frames are told apart by the setup token), so split messages lose their CRC-16; `UdpChecksum` is `CrcChecksum<LinkCrc32, true>`, a CRC-32/ISO-HDLC on every message with the frame prefix and head folded in, and every datagram starts with `UdpPrefix` (`SeqPrefix`: frame counter and session epoch), so lost, duplicated, reordered and previous-session datagrams are told apart. Hosts and devices built before can't talk to ones built after. `ww_link` depends on `ww_block` for the CRC-32 (one implementation for links and blocks): `std` turns on its `crc-table-slice16`, firmware picks a table with `ww_block`'s `crc-table-*` features (bitwise without).
- Link setup token (TR-15): `Message::GetDeviceInfo { token }` and `Message::LinkReady { token }` carry a `u64` (LE payload, none from older peers = 0), `DeviceInfo::token` and `LinkSetup::token` are new fields (`#[default = 0]`, so older peers still decode). The host picks a random non-zero token per connection, the device echoes it and the host drops everything before the reply carrying it, so that an answer left from a previous session is never taken for the new one. Wire change: hosts and devices without the token can't connect to ones with it (they get `Disconnect(IncompatibleVersion)` / `IncompatibleLinkVersion`). Match `Message::GetDeviceInfo { .. }` / `Message::LinkReady { .. }` instead of the unit variants.

### 🚀 Features

- `ClientMeta` and `ClientKind` (CLT-41): who a client is (program, app version, label, kind, pid, user, host, purpose, instance id), carried in the new `LinkSetup::client_meta: Option<ClientMeta>` (`#[default = None]`, appended: wire-compatible both ways, older peers read it as absent / ignore it). **Rust-source break:** `LinkSetup { .. }` literals need `client_meta`.

- `Framing` (TR-15): a medium's framer configuration as one type (`Head`, `Checksum`, `Tail`, `Prefix`), with `FramingTx` / `FramingRx` (and `FramingTxOwned` / `FramingRxOwned` with `std`) for its framers; `UsbFraming`, `UdpFraming` and `UsbSeqFraming` (USB with a `SeqPrefix` frame counter, a diagnostic). `UsbPrefix` / `UdpPrefix` next to the checksum aliases, `LinkCrc32` (the CRC-32 of `ww_block`), `USB_INTERFACE_PROTOCOL` (0x37) / `USB_INTERFACE_PROTOCOL_SEQ` (0x38) and `UsbFramingProtocol`: a USB device tells the host its framing in its interface protocol.
- Framer configuration for RTT: `RttHead`, `RttChecksum`, `RttTail` (`U2Head`, no checksum, no tail).
- Framer configuration for UDP: `UdpHead`, `UdpChecksum`, `UdpTail` (same as USB, one frame per datagram) and
  `UDP_MAX_DATAGRAM_LEN` (1452), the largest datagram a host sends.
- First version: sans-IO, `no_std` link layer — message kinds and their payloads carried over `ww_framer` frames.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.
