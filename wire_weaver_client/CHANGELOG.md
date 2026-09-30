## Unreleased

### 🐛 Fixes

- Connect intermittently failed (about half of the attempts on real hardware): when the transport's rx half and
  `TransportUp` arrived at the same time, the rx task could drop the rx half, closing the USB IN endpoint. The device
  reply was then never read, and a stale `DeviceInfo` was left for the next session.
- First request sent right after `connect()` returned could be rejected with "ignoring SendMessage while
  disconnected", because rx unblocks the caller before tx sees `LinkReady`. Requests arriving during link setup are
  now held and sent once the link is up, or failed with `Disconnected` if setup fails.
