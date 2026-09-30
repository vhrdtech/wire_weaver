## Unreleased

### 🚀 Features

- Per-resource API compatibility checks. If the device reports the same API hash as the client was generated with,
  nothing is checked. Otherwise, client and device introspection data are compared for every method, property,
  stream and trait (argument, return, property and stream types, property access, trait origin), following the
  evolution rules: e.g., an `Unsized` struct may gain trailing fields, but the reading side's extra fields need
  `#[default]`. Using a missing or incompatible resource fails locally with `Error::NotImplementedByDevice` or
  `Error::IncompatibleResource`, instead of sending a request the device would misread; all such resources are
  logged once on connect. If the device API is unknown (introspection disabled and not cached), `#[since]` of the
  resource is checked against the device's API version and fails with `Error::OlderProtocol` (was not checked before).
  Only absolute paths are checked, trait-client paths without an attachment base path are not.
  A trait or type whose definition is skipped on one side is checked by origin and crate version, and by signature
  when the versions are the same, which catches a definition changed without bumping its crate version.

- `snapshots` (re-export of `wire_weaver_snapshots`): API snapshots of `ww_global` and `ww_stdlib` crates, looked up
  by crate name and version with `snapshots::get()` or `snapshots::all()`. The per-resource compatibility check uses
  them to calculate the signature of an in-line trait or type that refers to skipped ones, which was not checked
  before.
- Introspection data downloaded from a device or loaded from `~/.wire_weaver/` has the traits and types the device
  left out (because they are known from snapshots) put back from snapshots, if their signatures match, so
  `Introspect::get()` and friends return the full API. Ones that are not known are logged and left skipped.
  The same is done for the client's own API embedded by codegen.

- `DeviceInfo` is filled from the USB API id interface string (falling back to the product string): API name,
  version, truncated hash and user label are known without opening the device, so `.user_label_eq()` and
  `.implements_api()` filters now work for USB. `DeviceInfo` gains `location` and implements `Display`.
- `list_usb_devices()` lists devices reporting an API id, `list_all_usb_devices()` lists all USB devices, neither
  opens a device.
- `ClientConfig::client_version()` sets the API crate name and version sent to the device during link setup.
- `ClientConfig::serial_contains()` selects a device by a part of its serial number.
- `ClientConfig::matches(&DeviceInfo)` checks a device against the config filters, e.g. to list only the devices a
  connection would consider.
- `DeviceInfo` gains `usb: Option<UsbLocation>` (bus id, port chain, VID and PID).
- `Introspect::download()`/`download_blocking()` (and `get*()` on a cache miss) save the downloaded API bundle to
  `~/.wire_weaver/`, so a device whose API was seen once is not downloaded again on the next connect. The bundle is
  only cached if its hash matches the one the device reported.
- `Introspect::with_timeout()` sets the introspection download timeout, restarted after each received chunk
  (defaults to the Commander's default timeout).
- `Stream::recv_all_bytes_timeout()`/`recv_all_bytes_timeout_blocking()` fail if no event arrives within the timeout.

### ⚠️ Breaking

- `Error` gains `NotImplementedByDevice` and `IncompatibleResource` variants, exhaustive matches need new arms.
- `IntrospectBundle` gains `sent_size`, the size of the introspection data as sent by the device, and
  `sent_api_bundle`, the introspection data as sent, before traits and types known from snapshots were put back into
  `api_bundle`; code constructing it must set both.

- `connect()`/`connect_blocking()` report why they failed instead of a generic "No devices found to connect to":
  - `Error::DeviceNotFound` is now a struct variant with the config's `filters` and the connected WireWeaver
    devices that did not match them (`unmatched`), both printed in the message. Match it with
    `Error::DeviceNotFound { .. }`.
  - New `Error::ConnectFailed { device, reason }` when the selected device fails to open or complete link setup;
    busy and (on Linux) permission denied errors include a hint on the likely cause.
  - New `Error::NoTransportSelected` when the config selects no transport (e.g., `.usb()` is missing).
  - Failing to list USB devices is returned as an error instead of being ignored.
- Device filters of different kinds are now all required instead of any one being enough, e.g.
  `.usb_vid_pid(..).serial_eq(..)` selects only the device with that serial, not every device with that VID:PID.
  Filters of the same kind are still alternatives. Configs relying on the old "any filter" behavior must drop the
  filters that shouldn't restrict the selection.
- `.usb()` without any filters now selects the only connected USB device reporting a WireWeaver API id, instead of
  selecting nothing. Without `.usb_vid_pid()` or `.usb_port_chain()`, devices not reporting an API id are not
  considered.
- `DeviceInfo` has a new public field `usb`, code constructing it must set it.
- `Introspect::download_promise()` is renamed to `Introspect::get_promise()`, since it checks the cache first like
  `get()`. It now resolves to an error if the device has introspection disabled, instead of a deserialization error.

### 🐛 Fixes

- `connect()` returns `Error::AmbiguousDeviceChoice` listing the matched devices one per line, instead of printing
  them to stdout in Debug format and returning a generic "Ambiguous device choice" error.
- Introspection cache lookup falls back to the bundle without doc strings when the one with doc strings is not
  cached, and matches exact file names instead of any name containing the hash.
- A missing `~/.wire_weaver/` directory or an empty device API hash is a silent cache miss instead of a warning on
  every connect.
- Introspection download no longer hangs `connect()` forever if the device never answers, it times out instead.
- Failing to download introspection data during `connect()` is logged instead of silently ignored.
- `PreparedDisconnect::asynch()`/`blocking()`/`keep_streams()`/`keep_streams_blocking()` return only after the
  transport is closed and the USB interface is released. Connecting to the same device right after a disconnect
  used to fail with "interface is busy (errno 16)". Queued outgoing USB packets (including the final Disconnect) get
  up to 100 ms to go out before they are cancelled.

- Connect intermittently failed (about half of the attempts on real hardware): when the transport's rx half and
  `TransportUp` arrived at the same time, the rx task could drop the rx half, closing the USB IN endpoint. The device
  reply was then never read, and a stale `DeviceInfo` was left for the next session.
- First request sent right after `connect()` returned could be rejected with "ignoring SendMessage while
  disconnected", because rx unblocks the caller before tx sees `LinkReady`. Requests arriving during link setup are
  now held and sent once the link is up, or failed with `Disconnected` if setup fails.
- Generated clients never sent their API version (empty `crate_id` in `LinkSetup`), so they were treated as dynamic
  clients and version compatibility was not checked on either side. It is now set automatically.
