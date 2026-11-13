# ww_link changelog

## Unreleased

### ⚠️ Breaking

- The hello goes outside the framer (EVO-23): `Framing` gains `hello_frame(&Hello, &mut [u8; hello::HELLO_FRAME_MAX]) -> Option<usize>` and `find_hello(&[u8]) -> Option<Hello>` (defaults: None, the hello goes through the framer); `UsbFraming` and `UsbSeqFraming` write and find the frozen USB frame, without a frame prefix. `hello::usb::find(packet)` lost its prefix argument and reads the payload length from the head. Custom `Framing` impls compile unchanged.
- The hello frozen on UDP, RTT and WebSocket too (EVO-23): `hello::udp` (the head, the payload and a CRC-32, without the frame prefix), `hello::rtt` (the USB bytes, found anywhere in a stream: `find(stream, role)` answers the hello and where it lies), `hello::ws` (kind byte 4 and the payload), each with `frame` and `find`, and `hello::frame_head(len)`. `UdpFraming` writes and finds the UDP hello outside the framer.
- `hello::rtt::Scanner`: finds the peer's hello in a stream read in chunks, also split across two reads, before the framer sees it (EVO-23). It compares four positions at a time with the hello's first two bytes and keeps a read's end only when it may still start a hello: about 4 instructions per byte on a Cortex-M3, 1.8 KB of flash in an RTT firmware.
- Frozen hello (EVO-23): `Kind::GetDeviceInfo` / `Message::GetDeviceInfo { token }` became `Kind::Hello` / `Message::Hello(Hello)`, sent by both sides. Its payload is plain bytes at fixed positions (new `hello` module: `Hello`, `Role`, `Generations`, `Layer`, `Mismatch`, `HelloError`, `GENERATION`, `hello::usb::{HEAD, frame, find}`): magic `WW`, role, generation bytes of framer, link, shrink_wrap and API model, the token. `DeviceInfo` now follows the device's hello and only when every generation matches. Match `Message::Hello(hello)` and use `hello.token` where you matched `GetDeviceInfo { token }`; wire break, every device and host is rebuilt together.
- `DeviceInfo` has a new field `image_facts: ImageFacts` (SRV-56): code built with a struct literal adds it. The wire stays compatible both ways, it is appended with a default: an old device reads as making no special choice, an old host steps over it.

### 🐛 Fixes

- `Kind::Data1` and `Kind::Data2` are documented as reserved for a later use, not free for applications (a device drops them).

### ⚠️ Breaking

- Per-medium framing (TR-15), wire change of USB and UDP: `UsbChecksum` is `NopChecksum` (USB checks, retries and orders every packet; stale frames are told apart by the setup token), so split messages lose their CRC-16; `UdpChecksum` is `CrcChecksum<LinkCrc32, true>`, a CRC-32/ISO-HDLC on every message with the frame prefix and head folded in, and every datagram starts with `UdpPrefix` (`SeqPrefix`: frame counter and session epoch), so lost, duplicated, reordered and previous-session datagrams are told apart. Hosts and devices built before can't talk to ones built after. `ww_link` depends on `ww_block` for the CRC-32 (one implementation for links and blocks): `std` turns on its `crc-table-slice16`, firmware picks a table with `ww_block`'s `crc-table-*` features (bitwise without).
- Link setup token (TR-15): `Message::GetDeviceInfo { token }` and `Message::LinkReady { token }` carry a `u64` (LE payload, none from older peers = 0), `DeviceInfo::token` and `LinkSetup::token` are new fields (`#[default = 0]`, so older peers still decode). The host picks a random non-zero token per connection, the device echoes it and the host drops everything before the reply carrying it, so that an answer left from a previous session is never taken for the new one. Wire change: hosts and devices without the token can't connect to ones with it (they get `Disconnect(IncompatibleVersion)` / `IncompatibleLinkVersion`). Match `Message::GetDeviceInfo { .. }` / `Message::LinkReady { .. }` instead of the unit variants.

### 🚀 Features

- `hello::GENERATION` is the line of the crate's wire version, declared in its `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"` (EVO-71), still 1. `ww_link` has a snapshot (`api_snapshots/ww_link_0_5_0.ron`) checked by `tests/stdlib_evolution`, so a break of `DeviceInfo`, `LinkSetup` or `ClientMeta` asks for `w2.0`, and with it the hello's link generation 2.
- The wire version of the user API in link setup (EVO-25): `DeviceInfo::user_api_wire` and `LinkSetup::host_user_wire`, both `Option<WireVersion>`, appended with `#[default = None]` (wire-compatible both ways: an older peer reads as `None` and steps over the field). `WireVersion` is re-exported. **Rust-source break:** `DeviceInfo { .. }` and `LinkSetup { .. }` literals add the field.
- `ClientMeta` and `ClientKind` (CLT-41): who a client is (program, app version, label, kind, pid, user, host, purpose, instance id), carried in the new `LinkSetup::client_meta: Option<ClientMeta>` (`#[default = None]`, appended: wire-compatible both ways, older peers read it as absent / ignore it). **Rust-source break:** `LinkSetup { .. }` literals need `client_meta`.

- `Framing` (TR-15): a medium's framer configuration as one type (`Head`, `Checksum`, `Tail`, `Prefix`), with `FramingTx` / `FramingRx` (and `FramingTxOwned` / `FramingRxOwned` with `std`) for its framers; `UsbFraming`, `UdpFraming` and `UsbSeqFraming` (USB with a `SeqPrefix` frame counter, a diagnostic). `UsbPrefix` / `UdpPrefix` next to the checksum aliases, `LinkCrc32` (the CRC-32 of `ww_block`), `USB_INTERFACE_PROTOCOL` (0x37) / `USB_INTERFACE_PROTOCOL_SEQ` (0x38) and `UsbFramingProtocol`: a USB device tells the host its framing in its interface protocol.
- USB vendor descriptor constants (TR-23), moved out of `wire_weaver_usb_embassy` so an embassy-free USB stack (`bedrock_usb`) writes the same bytes: `CUSTOM_DESCRIPTOR_TYPE_VENDOR_SPECIFIC_SELF_ID_VERSION` / `_USER_CRATE` / `_USER_VERSION` (0x40-0x42), `USB_DEVICE_CLASS_GUID`, and `const fn`s `usb_self_id_version_descriptor` / `usb_version_descriptor` that encode their payloads (a 4-byte crate tag plus major.minor.patch, each saturated to `u8::MAX`). `wire_weaver_usb_embassy` uses them; its wire bytes are unchanged.
- Framer configuration for RTT: `RttHead`, `RttChecksum`, `RttTail` (`U2Head`, no checksum, no tail).
- Framer configuration for UDP: `UdpHead`, `UdpChecksum`, `UdpTail` (same as USB, one frame per datagram) and
  `UDP_MAX_DATAGRAM_LEN` (1452), the largest datagram a host sends.
- First version: sans-IO, `no_std` link layer — message kinds and their payloads carried over `ww_framer` frames.
- `Generations` implements `Display` (EVO-34): `framer=1 link=1 shrink_wrap=1 api_model=1`, for the hello generations a connection agreed on, shown by `ww info`, `ww tap` and the Python `dev.info`.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.
