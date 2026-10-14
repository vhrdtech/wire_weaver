## Unreleased

### ⚠️ Breaking

- `Stream::open()` / `open_blocking()` are gone: a stream is opened by its generated call (API-31),
  `client.<name>().call().await` returns it. For `stream!` (`()` result) opening returns the stream itself, not
  `((), stream)`. `DynResource::stream()` opens plain streams with `StreamOpen`; `DynStream::open()` is gone. Stream
  functions and counted / reliable streams in the API model are `ApiItemKind::StreamFn`: the compatibility check
  compares their delivery, item type, arguments and result, the API diff prints them as `#[stream(T, kind)] fn`.
- The raw stream event `StreamEvent` (`Data` / `Sideband` / `Connected` / `Disconnected`, from `Sink::recv_any` and
  in `StreamError::UnexpectedEvent`) is now `RawStreamEvent`; `StreamEvent<T>` is the event of a counted or reliable
  stream (CLT-23). Code that named it: rename. It has a new variant, `Dropped { events, bytes }` (also in
  `TypedStreamEvent`): exhaustive matches need an arm for it.
- Per-stream queues are bounded (CLT-23): a plain stream (or a property observer) that isn't read for a while
  drops the newest data beyond `ClientConfig::stream_queue_bytes` (4 MiB by default) instead of growing the host's
  memory without limit; `Stream::dropped()` counts them, `recv_any()` reports them once as `Dropped`, `recv()` skips
  that report. Introspection data is never dropped.

### 🚀 Features

- Counted and reliable streams on the host (CLT-23, API-30), see `docs/api/stream_reliability.md`:
  `Commander::prepare_stream_open::<R, S>(path, delivery, args)` opens any stream by calling its stream function
  (`RequestKind::StreamOpen`), `prepare_stream_resume(path, delivery, cursor)` resumes one; `PreparedStreamOpen`
  (`call()` / `blocking_call()`, `with_timeout`, `ack_mode`, `window`, `queue_bytes`) returns the function's result
  with the handle (`StreamOpenResult`: the handle alone, or `Result<(A, S), E>`). `PositionedStream<T>`: `recv_event()` with
  `StreamEvent::Item { pos, units, t, run, value, .. }` / `Gap { at, units, cause, run }` / `Resumed` /
  `BackfillDone` / `Disconnected` / `Closed`, `recv()` for values only, blocking and `try_` variants, `split()` into
  the live and the backfill run, `cursor()`, `ack(pos)` (`AckMode::Explicit`), `stats()` (`StreamStats`), `close()`.
  The event loop runs each stream's `ww_stream::rx::Receiver`: positions, gaps by cause, duplicates, `Ack` and
  `ResendFrom` for reliable streams, the device's `Query`. `ClientConfig::stream_queue_bytes` bounds what a stream
  queues (`DEFAULT_STREAM_QUEUE_BYTES`, 4 MiB). After `disconnect().keep_streams()` the handles of counted and
  reliable streams wait instead of ending; `Commander::resume_streams()` (and `_blocking`) resumes every one of them
  from its cursor on the next connection (the reconnect itself is EX-10), the handle sees `Resumed` and goes on.

- `Delta`, `DeltaOfDelta` and `XorFloat` in APIs (SW-50): the dynamic client reads and writes them as lists of
  numbers; the evolution checker (`ww api check`) and the connect-time check report another codec or element type,
  or a plain `Vec` instead, as breaking.
- Counted and reliable streams in introspection data (API-30): the compatibility check and API diffs know
  `ApiItemKind::PositionedStream` (a delivery kind that differs between client and device is incompatible), the
  dynamic client lists them as streams.
- Follow a device across a reboot into other firmware (CLT-21), `follow` module: `Follow::gone()` / `back()` /
  `back_in_bootsel()` / `back_in_wire_weaver()` wait for the device with the same serial number to go away and come
  back as what is expected, on any `follow::Bus` (`UsbBus` for real devices); `Found` tells a WireWeaver device, an
  RP2040 / RP2350 boot ROM in BOOTSEL mode and anything else apart; a timeout lists what had the serial number then.
  `follow::rp2_update()` (`rp2_update_with()` on any bus and PICOBOOT IO) is the second half of an update in the
  RP2350 boot ROM: after `ww_dfu::BootControl::reboot(Updater)` it finds the BOOTSEL device by serial number, writes
  and verifies the image over PICOBOOT (`ww_picoboot`, re-exported), reboots it as `picotool load -x` does, and
  returns the firmware as it came back. `follow::rp2350_serial()` is the serial number a firmware reports so it can
  be followed (the chip id as the boot ROM shows it).
- Global trait calls on any device (CLT-10): `ClientConfig::traits_only()` connects without the user API check and
  without downloading the device's introspection, `DynClient::traits_only(|c| ..)` is a shortcut, and
  `DynClient::global_attachment(gid, trait_name)` gives an `Attachment` whose calls carry a global trait path the device
  resolves itself (API-20). A host built against a new API version can so reboot a device with an old one into its
  updater (`ww_dfu::BootControl`), or read its info traits. Absolute paths on such a connection fail with the new
  `Error::AbsolutePathInTraitsOnly`.
- Simulated USB for in-process devices: `in_process::usb_device(path, max_packet_size)` registers a device that
  speaks USB packets (`UsbSink` / `UsbSource`, wrapped in `ww_device::FramedTx` / `FramedRx` as real endpoints are),
  the host packs messages with the USB framer configuration of `ww_link`, exactly as over nusb. Connect with the same
  `ClientConfig::in_process_path()`. Used by the `ww_dfu` scenario tests.
- `MaxLen` bounds (SW-19) in compatibility checks: on connect a different bound is not an error (a longer value is
  rejected when it is sent); in `ww api check`, lowering or adding a bound is breaking and raising or removing one is
  compatible (`max length raised`). The dynamic client checks bounds of arguments and sink writes.
- `Sink::send_owned()` for sinks of owned types that hold a `Vec` or `String` (`FooOwned`, which implement
  `SerializeShrinkWrapOwned`, not `SerializeShrinkWrap`); `send()` could not send them.
- Credit sinks (`#[credit] sink!(..)`, API-28, `docs/api/streams.md`), credit counted in bytes: `Sink::send()` /
  `send_blocking()` / `send_owned()` wait until the device granted room for all bytes of the value (values are never
  split), fail with the new `StreamError::ItemTooBig` when a value is bigger than the device's receive buffer (the
  first credit after `Open`), `try_send()` / `try_send_bytes()` return `false` instead (for promise
  style code), `flush()` / `flush_blocking()` / `try_flush()` wait until the device accepted everything, lost writes
  are sent again from the device's position (go-back-N), `close()` flushes first. `CreditConfig` (query interval and
  stall timeout) via `Sink::set_credit_config()`, `Sink::credit_progress()` (bytes) for progress bars. `DynSink` speaks it
  too. `Commander::prepare_credit_sink()` / `_blocking()`.
- `Sink::send_blocking()` for typed sinks, `Sink::try_send()`, `send_owned_blocking()`, `try_send_owned()`; values are
  no longer limited to 1024 bytes.
- Compatibility checks (device vs host on connect, and `ww api` evolution checks) treat `sized` and `self_describing`
  as the same layout: a type can move between the two without being reported as breaking (SW-35).
- `Sink::recv_sideband()`, `recv_sideband_blocking()`, `try_recv_sideband()` and `recv_any()` / `recv_any_blocking()`:
  sideband events from the device (replies to `Sink::sideband()` and events it sends on its own) are no longer
  dropped.
- Generated clients have `observe_<property>()` / `observe_<property>_blocking()` for `ro` and `rw` properties: a
  `Stream` of property updates the device sends as stream data on the property's path.
- WebSocket transport (`ws` feature, replaces the outdated `wire_weaver_net_host` crate):
  `ClientConfig::websocket_url("ws://host:port/path")` connects through the regular `PreparedConnection` flow (link
  setup, version check, introspection, timeouts, streams), same as USB or RTT. Each `ww_link` message is one binary
  WebSocket message, `[kind: u8][payload ..]`, without `ww_framer`; messages are buffered until the event loop flushes
  and Nagle is off. Only plain `ws://` for now, no `wss://` and no discovery. Selecting WebSocket without the
  feature fails with an explicit error.
- UDP transport (`udp` feature, replaces the outdated `wire_weaver_udp_link` crate): `ClientConfig::udp_addr("host:port")`
  connects through the regular `PreparedConnection` flow, same as USB or WebSocket. One `ww_framer` frame per
  datagram with `ww_link`'s UDP framer configuration, at most `ww_link::UDP_MAX_DATAGRAM_LEN` (1452 B) sent, any size
  received. Nothing is retransmitted: lost requests time out, lost stream events are gone. If nothing listens on the
  device port, connecting fails right away. No discovery. Selecting UDP without the feature fails with an explicit
  error.
- In-process transport (`in_process` feature): a device running in the same process (simulators, tests) registers
  a path with `in_process::device(path, max_message_len)` and serves `ww_device::Server` with the returned
  `DeviceTx` / `DeviceRx`, the host connects with `ClientConfig::in_process_path(..)` through the regular
  `PreparedConnection` flow (link setup, version check, introspection, timeouts, streams). `in_process::TokioClock`
  is a `ww_device::Clock` for devices on the host. Integration tests under `tests/` now run on it, against the real
  event loop.

- `DynResource`: use any device API known only at runtime (from introspection data or a saved bundle), without
  generated code. Walk it by names and indices (`root.child("periph")?.index(0)?.child("gain")?`), call methods, read
  and write properties with `ww_self::ValueOwned` values, open streams (`DynStream`) and sinks (`DynSink`), read valid
  indices of arrays; async and blocking variants. Bytes on the wire are the same as with a generated client.
  `DynResourceKind` tells what a resource is, with its argument, return and property types. `ww_numeric` is
  re-exported.
- RTT transport (`rtt` feature): `ClientConfig::rtt(target, speed_hz)` connects through a debug probe with probe-rs,
  to a device serving over RTT (`ww_device::rtt`). The probe is selected with VID:PID and serial filters placed
  after `rtt(..)`, filters before it describe the device (e.g., a driver crate's `default_config()`) and are not
  used for the probe; WireWeaver USB is not tried with RTT selected. Probe IO runs on a dedicated thread polling the
  `ww_up` / `ww_down` channels, the framing is the stream mode shared with the device (`RttHead` / `RttChecksum` /
  `RttTail` from `ww_link`). Selecting RTT without the feature fails with an explicit error instead of
  "no devices found".
  `ClientConfig::rtt_elf(path)` / `rtt_control_block_at(address)` skip scanning RAM for the RTT control block
  (~1.5 s for 144 KiB through an ST-LINK, tens of milliseconds with the address known); RAM is scanned anyway if the
  running firmware doesn't match.
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

- `evolution` module, the evolution checker: `evolution::compare(old, new)` compares two crate snapshots
  (`ww api save` bundles) and returns a `Report` with the kind of change (`Change::None`, `DocsOnly`, `Compatible` or
  `Breaking`), the reasons, and warnings (additions without `#[since]`). `Report::check_version()` fails if the new
  crate version doesn't bump the required position: breaking changes need the breaking position, any other change,
  doc comments included, the compatible one. Resources are compared both ways with the same rules as the
  per-resource compatibility check; renames and removals are breaking too, as they break Rust code.
- `evolution::diff(old, new)` lists every difference between two crate snapshots as `evolution::Difference`s (added,
  removed, changed, or doc comments changed), doc comments included, without classifying them.
  `Report::minimal_version()` gives the smallest version the new one has to be.
- Per-resource compatibility check: types may gain new fields in between the old ones, in their unused padding bits,
  if no old field moves and the new ones read all zero bits as a valid value. `Unsized` types always start at a byte
  boundary, `final_structure`, `self_describing` and `sized` ones are checked for every bit offset they can start at.
  Before, such fields were only compatible at the end of an `Unsized` type (with `#[default]`), and other types
  were incompatible whenever the number of fields differed.

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

- `Sink::open()`, `close()` (and `_blocking`) take `&mut self`, same for `DynSink`: on a credit sink they reset or
  flush its state.
- `DynResourceKind::Stream` has a `flow: Option<FlowControl>` field.
- `ClientConfig::websocket_addr(addr, port, path)` is replaced by `ClientConfig::websocket_url(url)`, which also
  takes host names. The unused `net` feature is removed, use `ws`.
- `ClientConfig::udp_addr(addr, port)` takes a single `"host:port"` string now (host names are resolved when
  connecting), e.g., `udp_addr(format!("{ip}:{port}"))`.

- `PreparedWrite::write_promise` returns `Promise<()>` instead of `Promise<E>`. `PreparedWrite<E>`'s `E` is now the
  property's user error type (`()` if it has none) instead of `Result<(), E>`, generated `write_*` functions changed
  accordingly. Code naming these types has to be updated, e.g., `Promise<Result<(), MyError>>` → `Promise<()>`.

- `SeqTy` is `u32` (was `u16`). Request seq numbers cycle through 1..=127, so they are serialized into 1 byte, bigger
  ones (up to `ClientConfig::max_seq()`, `DEFAULT_MAX_SEQ` = 2 097 151 by default, 3 bytes) are only used while
  all of these are in flight. `Command::Connect` gains `max_seq`. `Command::SendMessage` bytes must start with the
  5-byte seq placeholder of a serialized `ww_client_server::Request` (the unused part of it is dropped before
  sending), also for requests without an answer, which are now sent with seq 0 trimmed to 1 byte. Wire-incompatible
  with devices built against the previous `ww_client_server`.
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

- USB: endpoint addresses and the interface number are no longer hardcoded (`0x01` / `0x81` on interface 0). They are
  read from the active configuration descriptor. The client picks the interface with WireWeaver class/protocol
  (`0xFF` / `0x37`), or for devices not reporting it, the first interface with a bulk or interrupt IN/OUT pair.
  Composite devices where the WireWeaver interface isn't first, or gets other endpoint numbers, now connect.
- Stream and sink constructors (generated `<stream>()` / `<stream>_blocking()`, `observe_<property>()`,
  `Commander::prepare_stream` / `prepare_sink`, `DynStream`, `DynSink`) returned before the event loop started routing
  events to them, so events the device sent right after were sometimes dropped and a following `recv()` waited
  forever. They now return once the stream is registered.
- A stream sideband request (`open()`, `close()`, `sideband()` on `Stream` and `Sink`) kept its seq until the default
  timeout, even when the device replied with a sideband event: the reply was only routed to the stream. Its seq is
  now released when the reply arrives, so a burst of sideband requests no longer runs out of request IDs.
- `write_promise` never succeeded: the empty reply was deserialized as `Result<(), E>` and failed with
  `OutOfBoundsReadBool`. A user error (`property!(rw name: T, UserError)`) was deserialized as `Result<(), E>` as
  well and failed, it is now formatted with the user error type into `Error::RemoteErrorDes`. `read_promise` no longer
  deserializes a user error as the property type, it returns `Error::RemoteError` with the raw bytes, same as `read()`.

- A late reply to a timed out request is no longer taken for the answer to a new request that got the same seq: the
  seq of a timed out request stays in use until the late reply arrives (which is then dropped) or until 10 s after the
  request was sent (`LATE_REPLY_WINDOW`), its own timeout still fails the call on time.
- Evolution diff and padding reuse checks tell a relocated `#[flag]` (`TypeOwned::Flag`) from the `Option` or
  `Result` field with the same name, a flag is a 1-bit field in the layout.
- Connect failing because the transport itself could not be opened (e.g., USB interface busy) could report a dropped
  channel instead of the actual reason.
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
