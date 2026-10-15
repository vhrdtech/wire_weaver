# Changelog

The history of WireWeaver as a whole, from its first YAML sketches to today, newest first. For the exact changes of
a released or upcoming crate version, read that crate's own `CHANGELOG.md` (`wire_weaver/`, `wire_weaver_core/`,
`shrink_wrap/`, `ww_stdlib/*`, ...): they are the authoritative record from 0.4.0 on. This file summarizes, per
version or era, what was added and changed, and which ideas first appeared there and are still part of WireWeaver.

The project changed names several times: AML (2017-2019, before git), vhapi (2020), vhL with xPI and vhl-stdlib
(2021-2023), and wire_weaver with shrink_wrap since May 2024. The version strings below are the workspace versions
from `Cargo.toml` at the time; only 0.4.0 was published to crates.io.

## [Unreleased]

### Fixed

- Malformed multi requests from a host no longer panic a `multi_req = true` device (CLT-15): a `Range` or `Mask32`
  index, a path at the depth limit or a deferred method inside one are answered with errors. `MultiCall` and
  `MultiWrite` are dispatched as calls and writes (they ran as reads), each index answered on its own like in
  `MultiRead`.

## [0.5.0] - 2026-10-15

Workspace version 0.5.0 since 30 Sep 2026, tagged 15 Oct 2026, not on crates.io yet. See the `## Unreleased` sections of
the per-crate changelogs for details and migration notes. Highlights since 0.4.0:

### Added

- Observe requests (API-38): a host asks a trait level's properties to be sent on change, periodically or both,
  within a byte budget, and gets them as one `PropertyUpdate` event per level carrying any subset of them
  (`ww_client_server::observe`; the device and host sides follow as SRV-18 and CLT-28).
- `ww_stdlib` back in this repository (Feb 2026), grown to GPIO (low and high level), I2C, SPI, UART, ADC, PWM,
  1-Wire, timebase, counters, indication, board/firmware/user info, UID, DFU, logging, SI units, numeric values,
  date/time, versions and common errors.
- Runtime introspection: a device describes its own API (`ww_self` model, deduplicated trait arrays, API signature
  hash), the client caches API bundles locally and can decode any value dynamically; API snapshots of `ww_global` and
  `ww_stdlib` are embedded so devices don't have to send them (`wire_weaver_snapshots`).
- `Stream<T>` and `Sink<T>` in API traits, credit-based sinks, reliable streams, bounded queues.
- New transport stack: `ww_framer` (packs many small messages into one packet or splits a big one, fuzzed, recovers
  from lost frames), `ww_link` (link setup, device info, checksums), `ww_device` (sans-IO device side with an async
  and a blocking server) and `wire_weaver_client` (one host event loop generic over the transport: USB, RTT, WebSocket,
  UDP, in-process).
- `ww` CLI: device list, `api scaffold` for server handlers, `api tree`, introspection, API snapshots and an evolution
  diff between API versions, shell completions.
- Python client (`wire_weaver_py`) built on dynamic introspection.
- `shrink_wrap`: a C99 runtime with the same wire format, compressed time series (delta, delta-of-delta, XOR floats),
  `#[cfg]` on enum variants with stable discriminants, fixed-layout fast path, much smaller code size.
- `ww_dfu` firmware update API with end-to-end scenarios over simulated USB, UDP and WebSocket.
- Integration test crates per API feature under `tests/`, CI that fails on any warning, documentation site moved to
  Zensical, `FEATURES.md` roadmap with stable item IDs.

### Changed

- Server handlers return `RpcResult` / `GetResult` / `SetResult` and receive a request `Context` (sequence number,
  medium, event output for stream updates and deferred replies).
- Client methods all take `&self`; `Promise<T>` works as a future and in immediate-mode UIs.
- Request sequence numbers are variable length (`UVlq32`): 1 byte up to 127 instead of 2.

### ⚠️ Breaking

- Wire: the USB link was replaced by `ww_framer` / `ww_link`, and `ww_client_server` request and event sequence
  numbers changed from `u16` to `UVlq32`, so 0.4.0 devices and 0.5.0 hosts (or the other way round) don't talk to
  each other. Rebuild both sides.
- Rust API: server handler signatures changed with `Context` and the result types; the per-crate changelogs say what
  to change.

## [0.4.0] - 07 Jan 2026

The first crates.io release of `wire_weaver`, `wire_weaver_core`, `wire_weaver_derive`, `wire_weaver_usb_embassy`,
`ww_self`, `ww_global`, `ww_version`, `ww_numeric` and `ww_client_server`. `shrink_wrap` came first as a standalone
format: 0.1.0 on 21 Dec 2025, 0.1.1 and 0.1.2 on 07 Jan 2026. This section covers the work of 2025, after the
rewrite described in the next two sections.

### Added

- USB link (Jan-Feb 2025): frame builder with fragmented receive, management channel between host and device, link
  info and sender statistics, CRC, `nusb` on the host, embassy-usb class on the device, fuzzing; later automatic
  WinUSB on Windows, nusb 0.2 queues, loopback and speed tests.
- Methods with arguments and return types, `Option<T>`, `Result<T, E>`, constants, initial streams (Feb 2025).
- Properties with read/write, access modes and `property_model` (`get_set`, `value_on_changed`), deferred methods
  answered later by sequence number, request id 0 meaning "no reply" (Mar 2025).
- `use_async` flag, WebSocket and UDP hosts; the protocol (`ww_client_server`) and the GUI tool (`wire_weaver_tool`)
  merged into this repository (Mar 2025).
- `#[derive(ShrinkWrap)]` and `#[derive_shrink_wrap]`; `U1`..`U63`, `I2`..`I63`, `ww_repr` enums, external types,
  tuples and arrays, `RefBox`, `#[final_structure]` / `#[self_describing]` / `#[sized]` with const asserts,
  `#[default = ..]` on evolved fields, `#[owned]` types, `to_ww_bytes` / `from_ww_bytes` (Apr-Jun 2025).
- `#[ww_trait]` in separate files and crates, multiple API levels, `ww_impl!`, resource arrays of methods, properties,
  streams and traits, trait attachments on the client, global trait addressing through `ww_global` (Jun-Nov 2025).
- `ww_version` (`FullVersion`, `CompactVersion`, `full_version!()`), `ww_date_time`, `ww_si`, `ww_numeric`,
  `ww_can_bus`, `ww_dfu` (Jun 2025).
- Streams with a sideband channel, stream subscribe, blocking client, `connect_raw`, reserved resources, `ww` CLI,
  documentation site on GitHub Pages with API overview, evolution rules and addressing (Nov-Dec 2025).

### Changed

- `Nib16` replaced by `UNib32` (1 to 11 nibbles) as the variable length integer (May 2025).
- Client codegen reworked around `CommandSender`; only the high-level client is kept, generated recursively for traits
  (Jul-Aug 2025).
- `ProtocolInfo` replaced by `ww_version` types (Nov 2025).
- `shrink_wrap` and `ww_stdlib` split into their own repositories (Dec 2025); both came back in Feb 2026.

### Ideas that survived

- Every API crate's name and version is its global identity; `CompactVersion` ids are assigned once an API is
  published.
- Explicit trade-off per type between evolvability and size (`Unsized` by default, `final_structure`,
  `self_describing`, `sized`).

## wire_weaver birth - 09 May to Oct 2024

Version 0.4.0 in `Cargo.toml` from 09 May 2024 ("Prepare for rewrite from scratch"): the vhL language and its own
parser were dropped, API definitions became plain Rust parsed with `syn`, and the project got the name wire_weaver.

### Added

- Project structure: one core crate (AST, transform passes, codegen) and one proc-macro crate (10-13 May 2024).
- Embassy USB class and a proof of concept talking to a real USB device (Jun 2024), tested by streaming SD card data
  for integrity and speed; USB code moved into its own repositories for a while.
- `ShrinkWrap` derive for structs and enums, USB test class with interrupt endpoints (Jul 2024).
- AST refactor with `syn` conversion as one transform pass, multiple passes to resolve type dependencies, codegen from
  `build.rs` as an alternative to the macro, `Result<T, E>` with flags picked up automatically, doc comments collected
  (Jul 2024).
- `wire_weaver_api!` macro (01 Aug 2024): takes arguments, generates the API model and the server dispatcher, a proof
  of concept client, `client` / `server` flags, `defmt` and `tracing` features.
- Method arguments, `RefVec` of bytes (Aug 2024); `ww_repr` for `u4` enum discriminants, `PacketBuilder` (Oct 2024).

### Ideas that survived

- The API is a Rust trait, the server is code generated onto the user's struct, the transport stays separate.
- One definition produces both no_std device code and std host code.

## shrink_wrap - 15 May to Jul 2024

### Added

- `shrink_wrap` crate (15 May 2024): `BufWriter` / `BufReader` over a byte slice, no alloc, bit- and nibble-level
  writes.
- `Vlu16N` variable length numbers written forward and in reverse: sizes of unsized fields go to the end of the
  buffer, so data can be written in one pass without knowing its length up front.
- Structs (nested and unsized), enums with named and unnamed fields, `str` / `String`, `Vec` / `RefVec`, `#[default]`
  attribute, test suite (May-Jun 2024).

### Changed

- `Vlu16N` became `Nib16` (Jul 2024); enum discriminants count from zero.

### Ideas that survived

- Sizes written backwards from the end of the buffer, zero-copy borrowed types next to owned ones, sub-byte
  alignment: still the core of the `shrink_wrap` wire format.

## [0.3.0] vhL, part 3: client-server and Promise - Jan to Nov 2023

### Added

- `mvlb` codec (encode/decode) as a tokio codec, event filters with timeouts, filters dropped when a remote node goes
  offline, indexed resource paths (Jan 2023).
- TOML config for codegen, struct trait derives, `cli generate` into the configured directory (May 2023).
- WebSocket support with a sync WebSocket client (Jun 2023).
- `Promise` and `PromiseStream` helpers for clients, event handler registration, shared routes, client statistics,
  notifications on client connect/disconnect, warnings when a pending `Promise` is dropped (Jul-Nov 2023).

### Changed

- The xPI format refactored into a client-server model (Jun-Jul 2023): crates `wfd`, `wfs`, `rmvlb-codec`,
  `xpi-client-server`; request sequence numbers instead of node ids, subgroup handlers per server component.
- `vhl-stdlib-rust` renamed to `vhl-stdlib`, `-rust` suffixes dropped (20 Jun 2023).

### Ideas that survived

- `Promise<T>` for immediate-mode UIs, request sequence numbers, a client-server model instead of a node mesh.

## [0.3.0] vhL, part 2: xPI, vhl-stdlib and xwfd - May 2022 to Dec 2022

### Added

- xPI grammar (26 May 2022): resources, access by URI, indexing, calls; SI units and binary prefixes in the lexer.
- Parser rewrite (Jul 2022): enums, `autonum`, type aliases, attributes, functions, Pratt expression parsing, better
  errors.
- xPI parser done (Aug 2022): resource paths, generics, number bounds, trait impls, `+stream` and `observe` modifiers,
  `const` / `rw` / `ro` / `wo` access.
- `vhl-stdlib-rust` (its own repository, merged here on 19 Aug 2022): nibble and bit buffers, `Vlu4` variable length
  numbers, `Vlu4Vec` with in-place builders, xPI request/reply serialization without allocation.
- `xpi-rust` with the `xwfd` wire format and owned types (Sep 2022), `xpi-node` with TCP requests/replies, heartbeats,
  event filters and tracing (Oct 2022), a basic xPI TCP server example (Nov 2022).
- Codegen of a no_std call dispatcher for methods at any depth of the resource tree, rustfmt and colored output
  (Sep 2022).
- One common AST crate, a `Project` with multi-file error reports through `codespan-reporting`, AST transform passes
  such as resource arrays (Oct-Nov 2022).
- Wire formats documented (Apr 2022, updated Sep 2022).

### Changed

- Workspace moved to `vhl-core`, common dependencies into `[workspace.package]` (Nov 2022).

### Removed

- The old parser, after the new one covered it (Dec 2022).

### Ideas that survived

- Nibble-aligned, variable length encoding for small numbers and indices (today's `UNib32`).
- A no_std dispatcher generated from the API description, resource arrays, properties that can be observed, streams.
- Serialization straight into a caller-provided buffer, no allocation.

## [0.3.0] vhL, part 1: the language - 08 Nov 2021 to Jan 2022

### Added

- A new start on a `pest` grammar (Nov-Dec 2021): numbers, literals, expressions, structs, enums, attributes, doc
  comments; type names capitalized, identifiers lowercase.
- README describing the type system, serialization, API, UI, CLI and docs; a book directory (Nov 2021).
- AST parsing of enums, tuple fields and types (Dec 2021).
- `mtoken` and `mquote` (Jan 2022): a `quote!`-like macro for generating code in any language, first Rust codegen
  for enums and discrete numbers, a CLI crate. A Dart backend was tried and dropped.

### Changed

- `vhl-cli` renamed to `vhl`, the old parser moved aside as `parser_old` (08 Nov 2021).

### Ideas that survived

- Rust-like types with traits for examples, allowed values and validity checks, instead of YAML keys (sketched Dec
  2020).
- Register maps with ids, access modes, read side effects, value tables and resource arrays such as `/rxf{0..4}`
  (sketched May 2021).
- Typed configuration and status/frame streams with filters for bus devices, e.g. a CAN bridge (sketched Oct 2021).
- Number sets for sub-type numbers, e.g. CAN FD payload lengths `{0..=8, 12, ..., 64}` (sketched Feb 2024).

## [0.2.0] syn - 16 Aug 2020 to Sep 2020

### Changed

- Parsing switched from the hand-written `nom` lexer to `syn`; AST nodes for registers; `vhapi-cli` became
  `vhl-cli`.

### Added

- A register map with templated register arrays (`RXF{n}`) and bitfields, for a CAN controller.

### Ideas that survived

- Parsing API definitions with `syn`, which wire_weaver returned to in 2024.

## [0.1.0] vhapi - 24 Apr 2020 to May 2020

The first git commit (24 Apr 2020).

### Added

- AST, loader and types modules, resource name parsing (Apr 2020).
- A `nom`-based tokenizer with `TokenStream` / `TokenTree`, number, boolean, literal and resource declaration parsers
  (May 2020).
- `syntax.yaml`: a full type system (`iN` / `uN`, varint, strings, stream and future types), includes from git or a
  registry, features, interface / codec / protocol stacking (e.g. CRC-checked framing, multiplexing).
- A BMS chip register map with bitfields and value descriptions.

### Ideas that survived

- Arbitrary bit-width integers and variable length integers in the type system.
- Reusable API definitions pulled in from other packages (today: API crates and `ww_stdlib` on crates.io).
- Transport kept separate from the API, layers stacked on top of each other.

## AML: YAML API markup - Nov 2017 to 2019 (before git)

The first ancestor of WireWeaver: devices described in YAML, code generated from templates.

### Added

- A device described as a protocol over an interface (Nov 2017); sketches for a CAN gimbal motor driver (UAVCAN) and
  an LwM2M blinds controller over CoAP (UDP, Thread, UART).
- A hand-written C++ target for a USB RGB LED with a msgpack extension type and accessors (Nov 2017).
- `usbled.yaml` (Jan 2018): msgpack over Custom HID or over CRC-checked UART framing, Jinja generators for Qt
  (`Q_PROPERTY`) and bare-metal C++ headers.
- Versioned API checks (Nov 2018): the first compatibility idea, e.g. a property changing from `bool` to `u8` between
  two tagged versions.
- Interface and protocol data-flow blocks, property traits (persistent, observable, rate-limited, read-only), msgpack
  extension types, algebraic enums, IMU and LED strip examples (Feb 2019).
- BMS composition: a mini CoAP over CAN, BLE or UART, battery gauge and monitor chip register maps included as proxy
  resources (Mar 2019).
- Module APIs for a CNC/CAM application (geometry types, file manager over msgpack/WebSockets), a Qt header template
  with `NOTIFY` signals (Jun 2019), a motion API sketch over CoAP (Sep 2019).

### Ideas that survived

- One API description, code generated for both device and host sides.
- Properties with traits (observable, read-only, persistent): today's property models and access modes.
- Checking API versions for compatibility: today's evolution rules and checker.
- RPC over plain verbs instead of REST (Nov 2018).
- Register maps of other chips exposed through a device's API.
