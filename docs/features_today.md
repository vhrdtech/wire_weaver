---
icon: lucide/list-checks
---

<!-- Generated from FEATURES.md by `just features-page` (tests/features_page), don't edit by hand. -->

# What works today

What WireWeaver can do now, by area: the items of the [feature tracker](features.md) that are done, and the
ones being worked on. The tracker has the details, what is planned, and for which release.

## Wire format

<div class="ww-columns" markdown>

- Structs, plain enums and data enums
- `Option` and `Result`
- `String` and `Vec<T>`
- Fixed size arrays, tuples and the unit type `()`
- Bit-aligned integers
- Variable length numbers
- Borrowed and owned types
- Evolvable by default
- `TailBytes` / `TailBytesOwned`
- `EitherAnyVec`
- `OptionAnyVec`
- `validate = path` derive directive
- `ArrayString<N>` / `ArrayVec<T, N>`
- Alloc writer
- In-place builders
- Binary format documented
- Usable standalone
- `Delta` / `DeltaOfDelta`
- `XorFloat`
- Compressed sequences in APIs
- Smaller code on small CPUs
- No length FIFO code for types without lengths
- `RefVec` of plain numbers without the decode pass
- Latched reads of `U1`..`U64` / `I2`..`I64` fields
- First error in types that mix latched and other fields
- C99 runtime
- Fixed-layout fast path
- Bit fields and a variable last field in the fixed-layout fast path
- Fixed head, sequential tail (`head_tail`)
- `head_tail` views and variant writers
- Numbers through `fixed_layout::Le`, out of line on RISC-V
- `#[cfg]` on `head_tail` enum variants
- Byte-wise `memcpy`, measured, advice for very small targets
- Bit reads stay out of the lengths in the last byte
- Exact `ElementSize` of enums
- `UVlq64`
- Bounded lists and strings `MaxLen<T, N>`
- Handle end of buffer when reading evolved fields
- `FutureVersion` enum variant
- Bit-sized fields and variant payloads
- `#[future_version]` variant by default
- Size-of-enclosing-type field
- `#[derive_shrink_wrap(..)]` attribute
- Discriminant checks
- `#[default = ..]` for evolved fields
- `TailBytes` must be last
- Generic arguments on user field types
- Field type from a macro_rules `$T:ty` fragment
- `Display` from doc comments
- Sparse structs
- `#[cfg(..)]` on enum variants

</div>

**In progress:** Fuzz `shrink_wrap`, Compression wrappers.

[All 68 items of this area in the tracker](features.md#wire-format-shrink_wrap-sw)

## API model and codegen

<div class="ww-columns" markdown>

- Methods, properties, streams and sinks
- Arrays of resources
- Global traits
- Results
- Request `Context` in every handler
- Event, `EventKind` and error builders
- `match` on index slices
- `build.rs` codegen
- Full name chains
- Type paths from Cargo
- Sync and async servers, async / blocking / promise clients
- Credit-based flow control for sinks
- Stream functions
- Optional items
- Per-method error lists
- Stream functions with a borrowed error type
- Const items served from a value
- Observe requests

</div>

**In progress:** Size analysis, Trait paths on the server side, Counted and reliable streams, Generate C.

[All 39 items of this area in the tracker](features.md#api-model-and-codegen-api)

## Server / device side

<div class="ww-columns" markdown>

- `no_std`, no-alloc server
- Sans-IO `DeviceLink`
- Fully split TX and RX flows
- User handlers called straight from the RX loop
- Device descriptors
- Media
- UDP device medium
- Stream stores and backfill
- Persistent RAM store
- Observed properties on the device
- deferred-nested-levels Deferred return helpers on nested levels
- ECC-safe STM32 flash for stores (hardware check open)

</div>

[All 19 items of this area in the tracker](features.md#server-device-side-srv)

## Client / host side

<div class="ww-columns" markdown>

- `std` client
- Generic sans-IO event loop
- USB transport
- RTT transport
- WebSocket, UDP and in-process transports
- `DynClient`
- Introspection cache
- Compatibility check on connect
- Connection selectors
- One `?` per call
- `ww info` and `ww report`
- Observing properties on the host
- Multi read/write/call

</div>

**In progress:** Trait calls that work on any device, Follow a device across a reboot into other firmware, Stream positions, gaps and resume on the host.

[All 26 items of this area in the tracker](features.md#client-host-side-clt)

## Transports and protocols

<div class="ww-columns" markdown>

- `ww_framer`
- `ww_link`
- USB
- USB CDC-NCM

</div>

**In progress:** USB throughput on small MCUs.

[All 12 items of this area in the tracker](features.md#transports-and-protocols-tr)

## Introspection

<div class="ww-columns" markdown>

- API model AST
- Bundles
- Relocated `#[flag]` fields

</div>

[All 8 items of this area in the tracker](features.md#introspection-ww_self-int)

## Evolution, versioning and compatibility

<div class="ww-columns" markdown>

- Evolution rules
- SHA signatures
- Crate version recorded in bundles
- Global traits referenced by hash
- API snapshots
- Evolution checker and report
- Skipped definitions from another revision

</div>

[All 9 items of this area in the tracker](features.md#evolution-versioning-and-compatibility-evo)

## Tooling: CLI, GUI, Python

<div class="ww-columns" markdown>

- `ww` CLI
- `ww api save` / `check` / `diff`
- `ww api scaffold`
- Python module
- Runtime literals
- `ww usb-loopback` link test
- Dynamic observe
- `ww settings dump`

</div>

**In progress:** Dev GUI / web tool.

[All 12 items of this area in the tracker](features.md#tooling-cli-gui-python-tool)

## Standard library

<div class="ww-columns" markdown>

- `ww_client_server`
- `ww_version`
- `ww_si`
- `ww_date_time`
- `ww_gpio`
- Common errors
- `ww_pwm`
- `ww_timebase`
- `ww_adc`
- `ww_onewire`
- `ww_si` / `ww_numeric` extended
- `ww_firmware_info` / `ww_board_info` / `ww_uid` / `ww_user_info` device contract
- `ww_counters` on `cnt` words
- `reset-record-age`
- `ww_indication` lightweight rework
- `ww_can_bus` on the stdlib idioms
- `ww_gpio` port access
- `ww_log` defmt over reliable streams (hardware check open)
- `ww_settings` settings store (hardware check open)

</div>

**In progress:** `ww_gpio_hl`, Prototypes, `ww_dfu` firmware update API, IO trait family, `ww_logic_analyzer`, `ww_signal_generator`.

[All 33 items of this area in the tracker](features.md#standard-library-ww_stdlib-std)

## Examples

<div class="ww-columns" markdown>

- Host examples
- Serialization only
- MCU examples
- NCM UDP example
- Project layout
- Multi-property observe demo
- cnt counters in MCU examples
- `ww_settings` demo

</div>

[All 15 items of this area in the tracker](features.md#examples-ex)

## Documentation

<div class="ww-columns" markdown>

- Docs site
- Reading levels and a SerDes section by level

</div>

**In progress:** Docs for `ww_stdlib` crates and resource doc comments.

[All 7 items of this area in the tracker](features.md#documentation-doc)

## Testing, CI and performance

<div class="ww-columns" markdown>

- CI on GitHub Actions
- Integration tests
- Device side end-to-end tests
- Timeout tests
- Method tests
- Property tests
- Stream tests
- Framer fuzzing
- `shrink_wrap` code size bench

</div>

**In progress:** Remaining test coverage, Binary size tracking, Power cuts against a flash store on STM32.

[All 20 items of this area in the tracker](features.md#testing-ci-and-performance-test)
