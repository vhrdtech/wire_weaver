# ww_link changelog

## Unreleased

### 🚀 Features

- Framer configuration for RTT: `RttHead`, `RttChecksum`, `RttTail` (`U2Head`, no checksum, no tail).
- Framer configuration for UDP: `UdpHead`, `UdpChecksum`, `UdpTail` (same as USB, one frame per datagram) and
  `UDP_MAX_DATAGRAM_LEN` (1452), the largest datagram a host sends.
- First version: sans-IO, `no_std` link layer — message kinds and their payloads carried over `ww_framer` frames.
