## 0.2.0 - unreleased

### ⚠️ Breaking

- USB packets carry no CRC-16 on split messages anymore (`ww_link::UsbFraming`, TR-15): a wire change, devices and hosts must both be built with it. `seq-prefix` feature: a frame counter at the start of every packet (`ww_link::UsbSeqFraming`) as a diagnostic, told to the host by the interface protocol (`USB_PROTOCOL_WIRE_WEAVER` is 0x38 then, 0x37 without); `UsbFraming` is the framing in use.
- `ServerBuffers` has an event scratch buffer, `MAX_MESSAGE_LEN` bytes more, used to serialize events sent from
  handlers and through `server.sink()`. `UsbServer` takes an optional medium type (`UsbServer<'d, D, M = ()>`), set
  with `server.with_medium(..)`.
- Event loop moved into user code: `usb_init()` returns `(UsbDevice, UsbServer)`, where `UsbServer` is a
  `ww_device::Server` with cancel-safe `wait()` and `handle()`, plus `run()` for the simple case.
  Other async sources are selected on alongside `wait()`, stream updates are sent through `server.sink()`.
- `send_updates()` and the `()` notification channel are removed.
- Link layer is `ww_link` over `ww_framer` (wire compatible with the current `wire_weaver_client`),
  `wire_weaver_usb_link` is no longer used.
- `usb_init()` takes a `ww_device::LinkConfig` instead of separate versions and API hash; `UsbTimings` has no ping period.
- `usb_init()` and `WireWeaverClass::new()` take an `api_id` (`server_impl::API_ID`, or one with a user label from
  `wire_weaver::api_id::with_label()`), `WireWeaverClass::new()` also takes a `&mut State`.
- `UsbBuffers::control` grows to 256 bytes to fit the longest (126-character) string descriptor.

### 🚀 Features

- `usb_init_with_ids()` and `usb_init_with_endpoints_with_ids()`: same as `usb_init()` / `usb_init_with_endpoints()`
  with a `UsbIds { vid, pid }` argument to set the device's USB VID:PID, which `config_mut` cannot do
  (`Config::vendor_id`/`product_id` are `pub(crate)` in embassy-usb 0.6). `usb_init()` and
  `usb_init_with_endpoints()` are unchanged, keep defaulting to the shared `0xc0de:0xcafe` test ID, and are now
  implemented in terms of the `_with_ids` variants (TR-17).
- `WireWeaverClass::with_endpoints()`: same as `new()` with the OUT and IN endpoints allocated by a closure on the class's interface, e.g. double-buffered bulk endpoints of newer embassy-usb / embassy-stm32 (2x the throughput on an STM32G0B1). `new()` is unchanged (TR-12).
- `usb_init_with_endpoints()`: same as `usb_init()` with the OUT and IN endpoints allocated by a closure, for
  callers of the single-function `usb_init()` convenience who can't reach `WireWeaverClass::with_endpoints()`
  directly because `UsbBuffers`' fields are private. `usb_init()` is unchanged, and now implemented in terms of
  it (TR-12).

- API id served as the WireWeaver interface string, so hosts can identify the device, its API and user label without
  opening it.
- `WireWeaverClass::into_server()`, to set up the server manually alongside other USB classes.
- `MAX_MESSAGE_LEN` is exactly the maximum message length reported to the host, independent of `MAX_USB_PACKET_LEN`
  (also when a Bulk endpoint is capped at 512).
- Host that restarted without disconnecting is detected, device-side peer timeout.
- Generic error reply when the backend cannot process a request at all.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.
- `usb_init()`'s doc no longer says to set vid/pid in `config_mut` (it can't): points at `usb_init_with_ids()`
  instead (TR-17).

## 0.1.0 - 07 Jan 2026

### 🚀 Features

- Harden USB link implementation.
- USB init and event loop.
- send_updates and improved event loop timer handling.
- USB loopback test.
- Use nusb queue, update to nusb 0.2.