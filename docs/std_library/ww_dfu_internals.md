---
level: nomicon
---

# ww_dfu: transcripts and chunky

How the exchanges on the [scenario pages](ww_dfu.md#the-scenarios) are recorded and checked, and the chunky records
`ww_dfu` carries as raw bytes.

## About the transcripts and their tests

Every transcript on the scenario pages ([A/B](ww_dfu_ab.md), [one slot](ww_dfu_one_slot.md)) is recorded by its test in `tests/dfu_scenarios` (`src/scenarios.rs`) and checked
against the file included there: a test fails when the exchange changes, until `just dfu-transcripts` writes the new
one and the change is looked at. So the bytes here are the bytes the current code sends.

- **Two views.** The readable one decodes each `ww_link` message with the API model of the firmware that runs:
  method, arguments, result, sink writes (`⇐`) and stream items (`⇒`), credit and sideband events, with each
  message's size and first bytes. The wire view shows every unit of the medium: USB packets, UDP datagrams or
  WebSocket frames. Both are taken on the device side.
- **Left out:** link setup (`Nop`, `GetDeviceInfo`, `LinkSetup`, `LinkReady`, the WebSocket upgrade), which is the
  same on every connection and carries crate versions that change with every release, and pings. WebSocket frames from
  the host are masked with a random key; they are shown unmasked with the key zeroed.
- **The device** is a simulation with real firmware logic on a simulated flash: two banks with NOR semantics (programming
  only clears bits), 8 chunks of 64 bytes, erase blocks of 128 bytes, a plan area, an apply journal, and power cuts
  injected at a given step. It runs as a task next to the host on one single-threaded runtime, as firmware runs on one
  core, so every run of a scenario interleaves host and device the same way (over the simulated USB the clock is paused
  as well, timeouts don't depend on the machine's load). A reset restarts the device task on the same link endpoint,
  running the bootloader logic first; the host reconnects as it would to a real device. The firmwares are generated `ww_dfu` servers
  (`tests/dfu_app_api`, `tests/dfu_lean_app_api`, `tests/dfu_updater_api`).
- **The plan format** is a small stand-in for chunky's (a 35-byte header with a toy signature; steps `01 block`,
  `02 dst src hash`, `03 dst hash len data`), so the step bytes in the transcripts are readable. Hashes are SHA-256
  truncated to 8 bytes, as in chunky's format 2. The real records are in
  [The other side: chunky](#the-other-side-chunky).
- **The links** are the real ones from `ww_device` up: WebSocket and UDP on localhost through `wire_weaver_client`'s
  `ws` and `udp` transports; USB is simulated by `wire_weaver_client::in_process::usb_device`, which runs the real USB
  framing on both ends with full-speed (64 B) or high-speed (512 B) packets, but not USB itself (enumeration, data
  toggles, NAKs). RTT is not used: with a debug probe attached a device is flashed directly, without a bootloader.
- **Changing a scenario's link** is one line, `const LINK: Link = ...` in its test (UART and CAN will join `Link`).

## The other side: chunky

`ww_dfu` is the transport half of an update. What it carries, and the code that checks and applies it on the device,
is [chunky](https://github.com/romixlab/chunky_mcu_updater) (`chunky_mcu_updater`, public).

- [`SPEC.md`](https://github.com/romixlab/chunky_mcu_updater/blob/main/SPEC.md) is the protocol in octets (a draft):
  the records, the receiver's checks in order, delivery modes, resume and segmentation, with annotated vectors. Its
  §10 describes this carrier.
- [`chunky_format/src/lib.rs`](https://github.com/romixlab/chunky_mcu_updater/blob/main/chunky_format/src/lib.rs)
  defines the records. They are `shrink_wrap` types with a fixed little endian layout, and the same bytes are read by
  Rust (with or without `BufReader`) and by a generated C reader:
    - **Descriptor** (`CKY2`): a 64-byte body (product, version, security counter, image length, geometry, SHA-256
      of the image) and a 64-byte seal over a domain string and exactly the body's bytes. The scheme is Ed25519, or
      SHA-256 only for an unsigned build, which a signed device refuses.
    - **Plan** (`CKP2`): a 145-byte header (geometry, step count, body length, the descriptor), then the steps.
      A step is a `head_tail` enum read through `Step::view`: ERASE is 3 bytes, COPY 13, WRITE 13 plus one chunk.
    - **Manifest** (`CKM2`): a 13-byte header and one entry per chunk, a hash (8 bytes of SHA-256), blank or
      unreadable.
- [`chunky/src/apply.rs`](https://github.com/romixlab/chunky_mcu_updater/blob/main/chunky/src/apply.rs) is the apply
  engine: it runs an untrusted plan against the DFU slot over `embedded_storage`, one step per call, checking the
  descriptor first, then every COPY source and WRITE hash, then the whole image.
- [`chunky_ww`](https://github.com/romixlab/chunky_mcu_updater/blob/main/chunky_ww/src/lib.rs) is the real device
  side of this API (scenario `2C-APP-STEP`, no_std): `ChunkyNode` answers `UpdateNode`, `BootControl`, `Inventory`
  and `Stage` with the apply engine and a boot backend (embassy-boot swap, hardware bank swap), so a firmware's
  generated handlers only forward. Its one error without a variant here is `Error::Custom(1)`
  (`chunky_ww::CUSTOM_NOT_BLANK`): the chunk a COPY or WRITE goes to is not blank, nothing was written, and the host
  reads the manifest again and plans again (SPEC §10).
- [`chunky_ww_host`](https://github.com/romixlab/chunky_mcu_updater/blob/main/chunky_ww_host/src/lib.rs) is the host
  side: node, manifest, plan, steps pipelined up to `StageLimits::in_flight` (`PreparedCall::send`), timeouts from the
  flash layout, reboot into the trial, the device's confirm or revert. [`ww dfu flash`](../cli_reference.md#ww-dfu-flash)
  is built on it, and its end-to-end tests run it against `chunky_ww` on chunky's simulated flash over the simulated
  USB of `in_process`.

The raw octets are these records: `Stage::begin` and `StageBulk::begin_bulk` take the 145 header bytes, `Stage::write`
carries one step's data, and the `StageBulk::plan` sink carries the body as segments. What a node answers with
`Error::BadSignature`, `BelowFloor`, `WrongGeometry` or `VerifyFailed` is what chunky decides; `WrongGeometry` is
what chunky and the spec call `GeometryMismatch`.

**Status.** Chunky does not implement the `ww_dfu` traits yet (chunky NET-1 is open): it is the device core and the
record format that `ww_dfu` carries, and the server side that exposes them as `UpdateNode`, `Stage` and the rest is
to come. Until then the exchanges on the scenario pages are recorded with the stand-in plan above.

