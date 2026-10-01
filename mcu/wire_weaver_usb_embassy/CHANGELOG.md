## 0.2.0 - unreleased

### ⚠️ Breaking

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

- API id served as the WireWeaver interface string, so hosts can identify the device, its API and user label without
  opening it.
- `WireWeaverClass::into_server()`, to set up the server manually alongside other USB classes.
- `MAX_MESSAGE_LEN` is exactly the maximum message length reported to the host, independent of `MAX_USB_PACKET_LEN`
  (also when a Bulk endpoint is capped at 512).
- Host that restarted without disconnecting is detected, device-side peer timeout.
- Generic error reply when the backend cannot process a request at all.

## 0.1.0 - 07 Jan 2026

### 🚀 Features

- Harden USB link implementation.
- USB init and event loop.
- send_updates and improved event loop timer handling.
- USB loopback test.
- Use nusb queue, update to nusb 0.2.