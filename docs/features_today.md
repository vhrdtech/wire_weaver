---
icon: lucide/list-checks
level: basic
---

<!-- Generated from FEATURES.md and tests/features_page/entries.md by `just features-page`, don't edit by hand. -->

# Features

WireWeaver lets you describe a device's API once, as a Rust trait with your own data types, and generates
both ends from it: the server on the device, `no_std` and without an allocator, and the client on the PC.
Calls, property reads and streams go over USB, RTT, UDP or WebSocket in `shrink_wrap`, a dense bit-packed
format that also works on its own. Newer firmware keeps working with older hosts and the other way round.

This page goes through what works today, area by area, in the order you are likely to meet them. Each area
starts with what it is for, then lists its features by [reading level](levels.md): Basic is what most
projects use, Advanced is for when the defaults are not enough, Nomicon is about internals and code size.
Features that work only in part are marked. What is planned, and the details of every item, are in the
[feature tracker](features.md).

## Wire format

`shrink_wrap` turns your Rust structs and enums into compact bytes and back, without an allocator and without
copying. Every message between host and device is made of it, and it works on its own as a dense storage format.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Structs and enums](serdes/derive.md)**: Your own structs, plain enums and enums that carry data, on the wire.
- **[One derive attribute](serdes/derive.md)**: `#[derive_shrink_wrap(..)]` with a few clear options and error messages that say what is wrong.
- **[`Option` and `Result`](types.md#option-and-result)**: Stored as a single flag bit, also inside lists.
- **[Strings and lists](types.md#strings-and-lists)**: `String` and `Vec<T>`, `Vec<u8>` as plain bytes.
- **[Arrays and tuples](types.md)**: Fixed-size arrays, tuples and `()`.
- **[Tuple and unit structs](types.md#tuple-structs)**: `struct Celsius(f32);` works like a named struct, with the same bytes; `struct Marker;` keeps room to grow fields later.
- **[Integers of any width](types.md#booleans-and-numbers)**: `u1` to `u32` and nibbles, packed bit after bit, so a 3-bit field takes 3 bits.
- **[Variable-length numbers](types.md)**: `UNib32` and `UVlq32`, where small values take a nibble or a byte.
- **[Zero-copy reads](serdes/derive.md#borrowed-owned-or-both)**: Strings and lists are read in place from the received buffer; an owned twin of each type is generated for values you keep.
- **[Evolvable by default](serdes/derive.md#evolving-types)**: Add fields later with `#[default = ..]`, and old and new code still read each other's data.
- **[Enum variants from a newer version](serdes/derive.md#enum-variants-from-a-newer-version)**: An older reader takes an unknown variant as `Unknown(u32)` instead of failing.
- **[Bounded lists and strings](types.md#bounded-lists-and-strings-maxlent-n)**: `MaxLen<T, N>` holds at most `N` elements, checked both ways, with a worst-case size known at compile time.
- **[Usable on its own](serdes/use_cases.md)**: A dense format for storing many small objects, no API needed.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Owned values without an allocator](serdes/special_types.md#arraystring-arrayvec-owned-values-without-an-allocator)**: `ArrayString<N>` and `ArrayVec<T, N>` on fixed arrays, same bytes as `String` and `Vec`.
- **[Bit-sized fields and payloads](serdes/showcase.md#bit-sized-fields-and-variant-payloads-a-7-or-10-bit-i2c-address)**: `U7`, `U10`, `I2` and friends in structs, enum variants and method arguments, e.g. a 7- or 10-bit I2C address in 8 or 11 bits.
- **[64-bit variable-length numbers](types.md)**: `UVlq64`, 1 to 10 bytes.
- **[Compressed number sequences](serdes/special_types.md#delta-deltaofdelta-and-xorfloat-compressed-sequences)**: `Delta` and `DeltaOfDelta` for timestamps and counters, `XorFloat` for slowly changing floats, anywhere in an API.
- **[Columnar series](serdes/series.md)**: `Series<T>`: a list of any struct written field by field, with deltas and bit packing per field, several times smaller than a `Vec<T>` of samples.
- **[Sparse structs](serdes/layouts.md#sparse-a-struct-of-optional-fields)**: A struct of optional fields stored as an `OptionAnyVec`, for "only what changed" updates.
- **[Checks on read](serdes/derive_reference.md#validate-path)**: `validate = path` rejects out-of-range values as they are read.
- **[`Display` from doc comments](serdes/derive_reference.md#display)**: Error enums print their doc lines, no hand-written `Display`.
- **[`#[cfg]` on enum variants](serdes/derive_reference.md#cfg-on-enum-variants)**: Compile a variant out; it keeps its number on the wire.
- **[Checked discriminants](serdes/derive_reference.md#ww_repr-repr-enums-only)**: Variant numbers must fit the chosen `ww_repr`, implicit ones count like in Rust.
- **[Generic field types](serdes/derive.md#type-mapping)**: `Wrapper<'i, T>` and types from `macro_rules` work as fields.
- **[Generic types](serdes/derive_reference.md#generics)**: Structs and enums with type and const parameters (`Timed<T>`, `Buf<const N: usize>`) and where clauses derive like any other type.
- **[`PhantomData` fields](serdes/derive_reference.md#generics)**: A zero-bit `PhantomData<U>` field carries a unit marker type (`Quantity<Volt>`) that needs no impls of its own.
- **[Growing writer](serdes/shrink_wrap.md)**: `BufWriterOwned` for hosts, when the size isn't known up front.
- **[Writing in place](serdes/showcase.md)**: Builders that write a value straight into the output buffer, and patching a field after the rest is written.
- **[Fixed layouts](serdes/layouts.md#fixed_layout-fields-at-fixed-offsets)**: Types with a fixed byte layout get fields at constant offsets, read and written without a parser, and a matching C header.
- **[Fixed head, sequential tail](serdes/layouts.md#head_tail-a-fixed-head-and-a-sequential-tail)**: `head_tail`, a format of its own for small firmware, with views that read fields on demand and a writer per enum variant.
- **[`shrink_wrap` in C](c.md)**: A C99 reader and writer of the same format, no `malloc`.

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Format documented bit by bit](serdes/wire_format.md)**: How every type is laid out, and why.
- **[Small code on small CPUs](serdes/code_size.md)**: About 20 % less code on Cortex-M0 and RISC-V for the same bytes on the wire.
- **[Number access tuned per CPU](serdes/code_size.md)**: Multi-byte numbers through one function on RISC-V, inline on Arm, whichever is smaller.
- **[Byte-wise `memcpy`](serdes/code_size.md#build-settings)**: Measured savings and downsides of a tiny `memcpy` for very small targets.
- **[Exact sizes of enums](serdes/size_assumptions.md#enums)**: `sized` enums report the discriminant plus one variant; variants of different sizes are a compile error.
- **[Strict bit reads](serdes/wire_format.md)**: Malformed data near the end of a buffer is an error, never a misread.
- **[Bytes to the end](types.md#special-types)**: `TailBytes` takes the rest of the buffer, written without a length; only allowed last.
- **[Size of the rest of a value](serdes/special_types.md#tailsizen-the-size-of-the-rest-of-a-value)**: `TailSize<N>`, filled in after writing, so a reader can skip a value or bound it.
- **[Lists written in place](serdes/special_types.md#refvecbuilder-lists-written-in-place)**: `RefVecBuilder` writes a list into the buffer one element at a time, from a loop or an iterator, with no slice and the same bytes.
- **[Sparse slots](serdes/special_types.md#optionanyvec-sparse-slots-of-any-types)**: `OptionAnyVec`, a list of optional values of any types where only the present ones take space.
- **[Lists of either-or values](serdes/special_types.md#eitheranyvec)**: `EitherAnyVec` holds one of two types per element, the flags packed by eight, e.g. a list of results.

<p class="ww-features__more" markdown>Details and what is planned: [Wire format](features.md#wire-format-shrink_wrap-sw) in the feature tracker.</p>

## API

You describe what a device offers as a Rust trait: methods to call, properties to read and write, streams of data
in either direction. WireWeaver generates the device's server and the host's client from it.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Methods, properties, streams and sinks](api/overview.md)**: All in one trait, with traits nested inside to group them.
- **[Sync or async](api/methods.md)**: Async and sync servers; async, blocking and promise-style clients, from the same trait.
- **[Results and errors](api/errors.md)**: Methods return a value, answer later or say "not implemented"; your own error types for methods and properties.
- **[Request context](api/methods.md#handler-context)**: Every handler knows who asked and can send events or answer later.
- **[Arrays of resources](api/arrays.md)**: `led[]` or `motor[]`, one definition for many instances, with the valid indices asked from the device.
- **[Shared traits](api/traits.md)**: `impl Gpio`, `impl Uart` from the standard library, so common tools work with your device.
- **[Constants](api/properties.md#const-items)**: `const NAME: T;` items, read once per connection and cached.
- **[Generic types](types.md#generic-types-in-an-api)**: `Timed<Ampere>`, `Window<4>` and your own generic structs and enums anywhere in an API, also in the dynamic client and Python.
- **[Type aliases](types.md#type-aliases)**: `type Volt = Quantity<VoltUnit>` and generic aliases anywhere in an API; tools show what they name.
- **[Types in modules](types.md#types-in-modules)**: API types can live in any `pub mod` or be re-exported under another name; generated code finds them.
- **[Self-referential types](types.md#self-referential-types-refbox)**: Trees and expressions (`RefBox`, a list of the type itself) anywhere in an API, also in the dynamic client and Python.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Optional items](api/optional_items.md)**: Leave out what a board doesn't have, with no flash spent, and clients know before they call.
- **[Error lists](api/methods.md#error-lists)**: Which errors each method can return, shown in docs and tools and checked in tests.
- **[Stream functions](api/streams.md#stream-functions)**: Open a stream with typed arguments and a result, like a method call.
- **[Observing properties](api/observing.md)**: Ask a device to send changes on change or periodically, within a byte budget.
- **[Size analysis](types.md#worst-case-size-at-compile-time)**: Worst-case request and reply sizes as constants, checked at compile time; properties and streams are next. <span class="ww-status">in progress</span>
- **[API docs in rustdoc](api/traits.md#in-rustdoc)**: `#[ww_trait]` also leaves a plain Rust trait, so methods, properties and streams show up on docs.rs. <span class="ww-status">in progress</span>
- **Code generation from `build.rs`**: `gen_server` / `gen_client` as an alternative to the macros.
- **[Generated C](c.md)**: The C runtime is there; generated C for an API's types and server is next. <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **Builders for events and errors**: Written straight into the output buffer.
- **[Dispatch on index slices](api/addressing.md)**: Requests are routed by `match` on their path, the base of multi requests.
- **[Names and paths that don't collide](api/folder_structure.md)**: Generated names carry the whole trait chain; type paths come from Cargo.
- **[Counted and reliable streams](api/stream_reliability.md)**: Exact counts of lost items; acknowledgements, resends and resume after a reconnect. <span class="ww-status">in progress</span>
- **[Credit sinks](api/credit_sinks.md)**: Flow control for data sent to a device, so its buffer never overflows, and lossy links resume where they stopped.
- **[Credit sinks without a size limit](api/stream_protocol.md#positions-wrap-at-256-mib)**: Sessions of any length, positions at most 4 bytes on the wire.
- **[Trait paths](api/addressing.md)**: Call a shared trait wherever it sits in a device's API. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [API model and codegen](features.md#api-model-and-codegen-api), [Documentation](features.md#documentation-doc) in the feature tracker.</p>

## Device side

The server is the code on the device that answers requests. It is generated from the trait, needs neither `std` nor an
allocator and does no IO itself, so it runs on any MCU and RTOS over the transport you choose.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[`no_std`, no allocator](api/methods.md#server-side)**: The generated server works on the smallest MCUs and does no IO itself.
- **[Your event loop](transport/overview.md)**: An async `Server` with `wait()` / `handle()` inside your own loop, or a blocking one.
- **[Media](transport/overview.md)**: USB (embassy), RTT, WebSocket and UDP, the last two also on `embassy-net`.
- **[Device descriptors](transport/usb.md)**: Serial number, API name, version and a user label, visible before connecting.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Deferred replies everywhere](api/methods.md#deferred)**: Answer a call later, also on nested trait levels.
- **[Observed properties](api/observing.md#the-device-side)**: The device sends property changes within the budget each host asked for.
- **[ECC-safe STM32 flash](api/ecc_flash.md)**: Reads a flash word broken by a power cut as zeros instead of crashing. <span class="ww-status">hardware check open</span>

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **Split send and receive**: Independent TX and RX paths for every transport; handlers called straight from the receive loop.
- **[Stream stores](api/stream_stores.md)**: Data a host hasn't acknowledged yet, kept in RAM or in a power-cut safe log on SD card or flash.
- **[Logs that survive a crash](api/stream_stores.md)**: A store in RAM that outlives a reset, for what happened just before a fault.

<p class="ww-features__more" markdown>Details and what is planned: [Server / device side](features.md#server-device-side-srv) in the feature tracker.</p>

## Host side

On the PC, a generated Rust client calls a device like a local library. A dynamic client does the same for any device
from the description the device sends of itself; the `ww` tool and Python are built on it.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Rust client](api/methods.md#client-side)**: Async, blocking or promise-style calls on a typed client.
- **[One `?` per call](api/methods.md#client-side)**: Device errors and link errors in one `Result`.
- **[Transports](transport/overview.md)**: USB, RTT through a debug probe, WebSocket, UDP, and in-process for tests.
- **[Picking a device](transport/overview.md#connecting)**: By serial number, label or path, with clear errors when none is found.
- **[Compatibility check on connect](evolution/rules.md)**: The client compares its API with the device's and tells what doesn't match.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Dynamic client](std_library/ww_self.md#dynamic-access-helpers)**: Talk to an API known only at runtime.
- **[Cached API descriptions](std_library/ww_self.md)**: A device's description is downloaded once, then found by its hash.
- **[Multi requests](api/multi_requests.md)**: Read many properties or call a method on many array elements in one request.
- **[Observing properties](api/observing.md#the-host-side)**: Typed subscriptions and a cache of the latest values, shared by everyone watching.
- **[`Series` without generated code](serdes/series.md#introspection-and-dynamic-clients)**: `ww`, Python and any dynamic client read a `Series<T>` from introspection alone, Python as numpy columns.
- **[Quantities without generated code](std_library/ww_si.md#in-ww-python-and-dynamic-clients)**: `ww` and Python show a `ww_si` value with its unit (`3300 mV`), Python as a quantity object with `to_si()`.
- **[Link setup timeout](cli.md)**: Connecting over a slow link (a debug probe) waits seconds, a fast one fails quickly, and you can set the time.
- **[Calls that work on any device](std_library/device_contract.md)**: Shared traits called whatever a device's own API version. <span class="ww-status">in progress</span>
- **[Following a reboot](transport/follow_reboot.md)**: Stay with a device while it reboots into its updater (RP2040 / RP2350 boot ROM so far). <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Sans-IO event loop](transport/overview.md#how-the-layers-fit)**: One event loop core, independent of the transport, with separate send and receive flows.
- **[Stream positions and resume](api/stream_reliability.md)**: Gaps reported exactly, streams resumed after a reconnect. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Client / host side](features.md#client-host-side-clt) in the feature tracker.</p>

## Transports

How the bytes get from one end to the other. Small messages are packed into one packet and big ones split, and a link
layer checks on connect that both ends speak the same protocol.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[USB, RTT, WebSocket, UDP](transport/overview.md)**: And in-process, also as simulated USB for tests.
- **[USB network adapter](transport/websocket.md)**: The device shows up as a USB network card with its own DHCP server, for WebSocket or UDP without drivers.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Framer](transport/ww_framer.md)**: Packs messages into packets with 1 byte of overhead, CRC and recovery from lost frames.
- **[Link layer](transport/ww_link.md)**: Link setup, device info and protocol version exchange.
- **[USB throughput on small MCUs](transport/usb_internals.md)**: Measured and tuned on an STM32G0 so far. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Transports and protocols](features.md#transports-and-protocols-tr) in the feature tracker.</p>

## Introspection

A device can send a description of its own API, so tools and scripts work with it without its source code.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Devices describe themselves](std_library/ww_self.md#the-api-model)**: The API model, sent by the device on request.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Moved flag bits](std_library/ww_self.md)**: Relocated `#[flag]` fields in the model and in dynamic values.

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Bundles](std_library/ww_self.md)**: The model with the crate versions and signatures of what it leaves out.

<p class="ww-features__more" markdown>Details and what is planned: [Introspection](features.md#introspection-ww_self-int) in the feature tracker.</p>

## Evolution

Devices and hosts are updated at different times. These keep old hosts working with new firmware and the other way
round, and tell you before a release when a change would break that.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Evolution rules](evolution/rules.md)**: What you can change in types and APIs, and which version to bump.
- **[Evolution checker](evolution/checker_tool.md)**: `ww api check` and `ww api diff` say whether a change is compatible.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[API identity](api/versioning.md)**: Every API and type has a signature; shared traits are referred to by hash.
- **[API snapshots](evolution/crate_layers.md)**: Descriptions of the standard library are left out of what a device sends and filled in by the host.
- **[Definitions from another revision](std_library/ww_self.md#the-definition-cache)**: A device built from an unpublished revision still connects, with a warning.

<p class="ww-features__more" markdown>Details and what is planned: [Evolution, versioning and compatibility](features.md#evolution-versioning-and-compatibility-evo) in the feature tracker.</p>

## Tools

A command line tool, a Python module and a GUI to work with devices and APIs without writing a client.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[`ww` command line tool](cli.md)**: List devices, show a device's API as a tree, with shell completions.
- **[Python](python.md)**: Any device from Python, its API taken from the device itself.
- **[Server scaffold](cli_reference.md#ww-api)**: `ww api scaffold` prints a stub for every handler a server needs.
- **[Device info and reports](cli_reference.md#ww-info)**: `ww info` for any device; `ww report` adds counters, logs and crash data in one file.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[API bundles](cli_reference.md#ww-api)**: `ww api save`, `check` and `diff`.
- **[Watching properties](cli_reference.md#ww-observe)**: `ww observe` and Python print updates as they arrive, with rates.
- **[Link test](cli_reference.md#ww-usb-loopback)**: `ww usb-loopback` measures round trips, losses and speed both ways.
- **[Settings from a flash dump](cli_reference.md#ww-settings)**: `ww settings dump` decodes a settings store without the device.
- **[Values from text](python_advanced.md)**: Typed values written as text and turned into bytes at runtime.
- **[Dev GUI](dev_tool.md)**: Shows the parsed API and the generated code side by side; a UI for any device is prototyped. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Tooling: CLI, GUI, Python](features.md#tooling-cli-gui-python-tool), [Client / host side](features.md#client-host-side-clt) in the feature tracker.</p>

## Standard library

Ready-made traits and types for what many devices have: units, time, GPIO and buses, logs, firmware info, updates.
They are shared, so a tool that knows one of them works with every device that implements it.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Device contract](std_library/device_contract.md)**: What every device answers: firmware, board, ids, user label and last reset.
- **[Common errors](std_library/ww_error.md)**: `CommonError` (`Busy`, `Timeout`, `InvalidArgument`, ...) shared by all APIs.
- **[Units and numbers](std_library/ww_si.md)**: `ww_si` quantities like volts, hertz and seconds, `ww_numeric` values.
- **[Units checked by the compiler](std_library/ww_si.md#unit-algebra)**: One generic quantity type with the unit in the type: `Volt * Ampere` is a `Watt`, `Volt + Ampere` doesn't build, `3300 mV` prints itself.
- **[Units known at run time](std_library/ww_si.md#units-known-only-at-run-time)**: A value whose unit is only known on the device, a configurable sensor channel, carries its unit and is shown like a typed one. <span class="ww-status">in progress</span>
- **[Date and time](std_library/ww_date_time.md)**: `ww_date_time`.
- **[IO family](std_library/io_family.md)**: GPIO, UART, I2C, SPI, ADC, PWM, 1-Wire and more, all in one style. <span class="ww-status">in progress</span>
- **[GPIO](std_library/ww_gpio.md)**: Pins and banks, and reading or writing several pins at one instant.
- **[CAN bus](std_library/ww_can_bus.md)**: From a simple bridge to a bus debugger.
- **[Logs](std_library/ww_log.md)**: `defmt` logs over a reliable stream, including what was logged before the host connected. <span class="ww-status">hardware check open</span>
- **[Indicators](std_library/ww_indication.md)**: LEDs off, essential or normal, and a blink to find a unit on the bench.
- **[Firmware updates](std_library/ww_dfu.md)**: `ww_dfu` for A/B, in-place and bootloader updates, tested with reboots and power cuts. <span class="ww-status">in progress</span>
- **[1-Wire](std_library/ww_onewire.md)**: Transactions and ROM search.
- **[More buses](std_library/ww_i2c.md)**: `ww_i2c`, `ww_uart`, `ww_memory` and the `ww_gpio_hl` helpers, usable with gaps. <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[ADC](std_library/ww_adc.md)**: Single reads, sample streams and triggered captures.
- **[PWM](std_library/ww_pwm.md)**: Outputs and input capture.
- **[Device time](std_library/ww_timebase.md)**: A clock with sync to a shared time source.
- **[Logic analyzer](std_library/ww_logic_analyzer.md)**: Triggered captures, transitions compressed. <span class="ww-status">in progress</span>
- **[Signal generator](std_library/ww_signal_generator.md)**: Replays captures, drives DAC channels. <span class="ww-status">in progress</span>
- **[Counters](std_library/ww_counters.md)**: A device's `cnt` counters, named on the host from its firmware file.
- **[Recorder control](std_library/ww_recorder.md)**: Status, format, recordings, markers and settings of a logger that writes to an SD card. <span class="ww-status">in progress</span>
- **[Age of a crash record](std_library/ww_firmware_info.md)**: How many boots ago the saved crash happened.
- **[Settings store](api/settings.md)**: A device's settings in its own flash, power-cut safe, readable by older and newer firmware. <span class="ww-status">hardware check open</span>
- **[Versions](std_library/ww_version.md)**: `ww_version`.

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Request and event model](std_library/ww_client_server.md)**: `ww_client_server`, the messages every call is made of.

<p class="ww-features__more" markdown>Details and what is planned: [Standard library](features.md#standard-library-ww_stdlib-std) in the feature tracker.</p>

## Examples

Working projects to start from: host programs, firmware for a few boards, and the crate layout to copy.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Project layout](api/folder_structure.md)**: An `<name>_api` crate with the trait and types, next to the code that serves and calls it.
- **[Host examples](examples/examples.md)**: Blinky, all GPIOs, UART, and blinky from Python.
- **[Firmware examples](examples/examples.md)**: STM32 Nucleo boards, RP2040 / RP2350 and QEMU, over USB, RTT, WebSocket and UDP.
- **[Serialization only](examples/examples.md)**: `shrink_wrap` without an API, and a comparison with other formats.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[UDP over a USB network adapter](examples/examples.md)**: An RP2 board and its host side.
- **[Motor controller demo](examples/motor_demo.md)**: 52 observed properties at three rates on a simulated motor.
- **[Counters in firmware](std_library/ww_counters.md)**: `cnt` counters in every probe-connected board example.
- **[Settings demo](api/settings.md)**: Settings that survive a reboot, on the host and on two Nucleo boards.

<p class="ww-features__more" markdown>Details and what is planned: [Examples](features.md#examples-ex) in the feature tracker.</p>

## Testing

How WireWeaver is kept working: every change runs through CI, against the real client and server code.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **Continuous integration**: Formatting, lints, docs, tests, semver checks and firmware builds on every change.
- **Integration tests**: Every API feature tested through the real host event loop and device server.
- **Fuzzing**: The framer and `shrink_wrap`'s readers fed random input; more targets to come. <span class="ww-status">in progress</span>
- **Test coverage**: A few remaining cases. <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Code size bench](serdes/code_size.md)**: Firmware images per case for RISC-V and Cortex-M, measured on every change.
- **Binary size tracking**: Flash and RAM per feature in CI. <span class="ww-status">in progress</span>
- **[Power cuts on STM32](api/ecc_flash.md)**: A flash store reset thousands of times mid-write on real hardware. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Testing, CI and performance](features.md#testing-ci-and-performance-test), [Wire format](features.md#wire-format-shrink_wrap-sw) in the feature tracker.</p>
