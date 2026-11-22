---
level: advanced
---

# Architecture: crates and repositories

WireWeaver is split over several repositories and many crates. The split follows one rule: a crate depends only on
its own layer and the layers below it. This page explains the layers, says what each repository holds, and groups
the crates of this repository by the job they do. Read it before adding a crate, moving code between crates, or
looking for where a piece of the framework lives.

## The layers

A **layer** is a group of crates with the same place in the stack. The stack has eight numbered layers, from the wire
format at the bottom to the shipped tools at the top:

| Layer | Name            | What it holds                                                                 | Repository                                      |
|-------|-----------------|-------------------------------------------------------------------------------|-------------------------------------------------|
| 0     | Wire format     | `shrink_wrap` and its derive macro                                            | [shrink_wrap](https://github.com/romixlab/shrink_wrap) |
| 1     | Units           | `exact_units`: units, `Fixed`, points, indices                                | [exact_units](https://github.com/romixlab/exact_units) |
| 2     | Base types      | `ww_si`, `ww_numeric`, `ww_version`, `ww_global`, `ww_error`, `ww_date_time`, `ww_keyed`, `ww_crc` | [ww_base](https://github.com/vhrdtech/ww_base) |
| 3     | Storage         | the tape format: `ww_tape` and its codecs                                     | [ww_tape](https://github.com/vhrdtech/ww_tape)  |
| 4     | API description | the API model, the derive and codegen macros, snapshots, the evolution checker | this repository                                 |
| 5     | Runtime and host | servers, clients, links, transports, the hub, Python, tests and examples     | this repository                                 |
| 6     | Standard library | reusable API traits: GPIO, I2C, SPI, UART, CAN, DFU, logs and more            | [ww_stdlib](https://github.com/vhrdtech/ww_stdlib) |
| 7     | Shipped tools   | the `ww` binary and the `wire_weaver` Python wheel                            | [ww_tools](https://github.com/vhrdtech/ww_tools) |

Layers 0 to 3 are taken by git, without a version pin in `Cargo.toml`: `Cargo.lock` records the commit, and
`cargo update -p shrink_wrap` (or `exact_units`, `ww_si`) moves it forward. Layers 6 and 7 build on this repository,
so nothing here depends on them. For example, a framework test that needs a GPIO-like trait uses a stand-in from
`tests/skipped_dep_api` instead of `ww_gpio`.

The map of every crate to its layer is `layers.toml` at the repository root. Two checks read it. `tests/layers_lint`
fails a crate that depends on a layer above its own, and an API crate whose dependencies reach the runtime.
spirit_level fails a crate that the map does not list. [Changing WireWeaver itself](evolution/crate_layers.md)
explains how each layer is versioned and why a change at the bottom travels through everything above it.

## The repositories

Each sibling repository has its own book or feature tracker, and its own tests and API snapshots.

| Repository | Holds |
|------------|-------|
| [shrink_wrap](https://github.com/romixlab/shrink_wrap) | The wire format: `shrink_wrap` (serializer, deserializer, the bit-packed buffer types) and `shrink_wrap_derive` (`#[derive_shrink_wrap]`), with its size bench and fuzz targets. Its pages form the [SerDes](serdes/shrink_wrap.md) section of this book. |
| [exact_units](https://github.com/romixlab/exact_units) | Typed numbers: unit markers, dimensions, `Fixed<U, P, T, B>`, scales, bounds, points, indices, rounding and customary units. It has its own book at [exact.vhrd.tech](https://exact.vhrd.tech/). |
| [ww_base](https://github.com/vhrdtech/ww_base) | The types every API uses: `ww_si` (re-exports `exact_units` with unit groups as features), `ww_numeric`, `ww_version`, `ww_global` (the registry of global trait ids), `ww_error`, `ww_date_time`, `ww_keyed` and `ww_crc`. |
| [ww_tape](https://github.com/vhrdtech/ww_tape) | The tape format for recording typed data over time: `ww_tape`, `ww_tape_codecs`, `ww_tape_fs`, `ww_tape_mux` and `ww_tape_summary`, with benches and fuzz targets. It depends on nothing of the framework. Its book is at [tape.vhrd.tech](https://tape.vhrd.tech/); [Tapes](tapes.md) says where tapes meet WireWeaver. |
| [ww_stdlib](https://github.com/vhrdtech/ww_stdlib) | The standard library: API traits and types for unrelated projects (`ww_gpio`, `ww_i2c`, `ww_spi`, `ww_uart`, `ww_can_bus`, `ww_dfu`, `ww_log`, `ww_board_info`, ...), the I2C and SPI slave engine `ww_bus_slave`, and `ww_std_cli`, the `ww` commands of its devices. |
| [ww_tools](https://github.com/vhrdtech/ww_tools) | What users install: the `ww` binary, the `wire_weaver` Python wheel, the API snapshot set both embed, the tap decoder `ww_tap_decoder` and `ww_tape_bundle`, which names tape tracks from an API bundle. |

## Crates of this repository

This repository holds layers 4 and 5. The generated table of every crate, with its layer and description, is
[CRATES.md](https://github.com/vhrdtech/wire_weaver/blob/master/CRATES.md); the groups below say what each crate is
for.

### Layer 4: API description

These crates describe an API without running it. An API crate depends on `ww_api` only, so it builds for any target
and pulls in no runtime.

| Crate | What it does |
|-------|--------------|
| `ww_api` | Everything an API crate needs: the prelude with the derive macros, `shrink_wrap`, `ww_error` and `ww_version`, and the `api_doc` markers. |
| `wire_weaver_derive` | `#[ww_trait]` and `#[ww_api_root]`, which turn an API trait into a documented Rust trait and version constants, and `ww_codegen!` and `ww_impl!`, which generate the server and client code. |
| `wire_weaver_core` | The code generator behind the macros: it parses an API's source and its dependencies into an AST, then emits server, client and type code. |
| `ww_self` | The API model as a WireWeaver type. A device sends it as introspection data; snapshots and the dynamic client read it. |
| `ww_snapshots` | Finds the API snapshot of a crate by name and version: in embedded sets, the per-user cache, Cargo's sources or a fetch. |
| `wire_weaver_evolution` | The evolution checker: compares an API crate with its saved snapshots and says which version position to bump. |
| `ww_lz4` | A streaming LZ4 block encoder for `no_std` and no allocator, used for compressed introspection data and streams. |

### Layer 5: device side

These crates run in firmware: `no_std`, no allocator, and big state in a `static`.

| Crate | What it does |
|-------|--------------|
| `wire_weaver` | The runtime that generated servers and clients use: the server traits, the handler `Context`, result types. It re-exports `ww_api`. |
| `ww_client_server` | The request and event encoding that every server and client speaks. |
| `ww_device` | The device side of a link: a sans-IO state machine and async and blocking servers, with RTT, WebSocket, UDP and `embassy-net` behind features. |
| `ww_stream` | Counted and reliable streams, observed properties and their stores. |
| `ww_settings`, `ww_settings_derive` | A power-cut safe settings store in NOR flash, with `#[derive(Settings)]`. |
| `ww_timebase` | The device clock API and time synchronization types. |
| `ww_trace_context` | W3C trace context, as requests carry it. |
| `mcu/wire_weaver_usb_embassy` | The USB device class on embassy-usb. |
| `mcu/ww_ecc_flash` | A NOR flash wrapper for STM32 that reads a word with an uncorrectable ECC error as zeros instead of faulting. |

### Layer 5: links and transports

These crates carry messages between a client and a server. Generated code does no IO, so the same API runs over any
of them.

| Crate | What it does |
|-------|--------------|
| `ww_link` | The link layer: version and buffer-size exchange before data flows, keep-alive pings, an explicit disconnect with a reason. |
| `ww_framer` | Packs many small messages into one packet and splits a big one across several. |
| `ww_byte_stream` | The connection preamble and message framing over byte streams such as TCP or a serial port. |
| `ww_local_socket` | Local sockets for programs on one computer: user-only Unix sockets and named pipes. |
| `ww_dns_sd` | Announces WebSocket servers on the local network over DNS-SD. |
| `ww_tap` | The tap hook: a transport reports its frames, messages and link events to it, for capture and decoding. |

### Layer 5: host side

These crates run on a computer. A host can be a client of a device, a server with an API of its own, or both.

| Crate | What it does |
|-------|--------------|
| `wire_weaver_client` | The client runtime: the event loop, the generated clients' backend, the dynamic client that needs no generated code, and the host transports (USB, RTT, WebSocket, UDP, local socket, in-process). |
| `wire_weaver_server` | The host server: one API served to many clients at once, on any async runtime. |
| `ww_calloop` | The host server on a calloop event loop. |
| `ww_hub`, `ww_hub_api` | The hub: one process per user that holds the devices and serves every program on the computer. |
| `ww_clock_sync` | Fits each device's clock against the host's and steers it onto UTC. |
| `wire_weaver_py` | The Python module: talks to any device, with its API built at run time from introspection data. |
| `wire_weaver_cli` | The generic `ww` commands and the registry a binary assembles them with. |
| `ww_plugin` | What an executable `ww` plugin (`ww-NAME`) needs. |
| `stm32_dfu_host` | A host for the STM32 system bootloader's USB DFU. |
| `shrink_wrap_c` | Builds the C runtime of the wire format and checks it against the Rust crate. |
| `wire_weaver_tool` | The egui viewer ([dev tool](dev_tool.md)). |

### Layer 5: examples, tests and support

- **`examples/`** holds paired crates: `<name>_api` with the trait and types, and `<name>` with the server and
  client. `blinky` is the smallest; `local_trait` shows one call site that works against a local struct or a remote
  client. [Examples](examples/examples.md) lists them.
- **`examples_mcu/`** holds firmware per board (STM32G0, STM32H7, RP2350) and the QEMU benches for instruction
  counts and flash size. Each is its own workspace.
- **`tests/`** holds one integration test crate per feature, each with a `<name>_api` companion. `tests_common` runs a
  generated server in a thread against the real host event loop. `tests/conformance` is the wire-format conformance
  matrix, and `tests/layers_lint` checks the layer map.
- **`fuzz/`** holds the fuzz targets of the framer, the link, introspection and series.
- **`ww_book`** assembles this book: it copies the pages of the sibling repositories into `docs/`.
- **`wire_weaver_client_benchmarks`** measures where a connection's time goes.
