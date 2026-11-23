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
- **[Signed variable-length numbers](serdes/showcase.md#unib32-variable-length-numbers-by-the-nibble)**: `INib32`, zigzag over `UNib32`: small negative values stay as short as small positive ones.
- **[Zero-copy reads](serdes/derive.md#borrowed-owned-or-both)**: Strings and lists are read in place from the received buffer; an owned twin of each type is generated for values you keep.
- **[Evolvable by default](serdes/derive.md#evolving-types)**: Add fields later with `#[default = ..]`, and old and new code still read each other's data.
- **[Enum variants from a newer version](serdes/derive.md#enum-variants-from-a-newer-version)**: An older reader takes an unknown variant as `Unknown(u32)` instead of failing.
- **[Bounded lists and strings](types.md#bounded-lists-and-strings-maxlent-n)**: `MaxLen<T, N>` holds at most `N` elements, checked both ways, with a worst-case size known at compile time.
- **[Usable on its own](serdes/use_cases.md)**: A dense format for storing many small objects, no API needed.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Owned values without an allocator](serdes/special_types.md#arraystring-arrayvec-owned-values-without-an-allocator)**: `ArrayString<N>` and `ArrayVec<T, N>` on fixed arrays, same bytes as `String` and `Vec`.
- **[Bit-sized fields and payloads](serdes/showcase.md#bit-sized-fields-and-variant-payloads-a-7-or-10-bit-i2c-address)**: `U7`, `U10`, `I2` and friends in structs, enum variants and method arguments, e.g. a 7- or 10-bit I2C address in 8 or 11 bits.
- **[64-bit variable-length numbers](types.md)**: `UVlq64`, 1 to 10 bytes.
- **[Columnar series](serdes/series.md)**: `Series<T>`: a list of numbers, timestamps, floats or any struct written field by field, with deltas and bit packing per field, several times smaller than a `Vec<T>` of samples.
- **[Sparse structs](serdes/layouts.md#sparse-a-struct-of-optional-fields)**: A struct of optional fields stored as an `OptionAnyVec`, for "only what changed" updates.
- **[Checks on read](serdes/derive_reference.md#validate-path)**: `validate = path` rejects out-of-range values as they are read.
- **[Refusals with numbers](serdes/derive_reference.md#validate-path)**: A `validate` hook can say which value it refused and the range it wanted; a host reader gets both, firmware pays nothing.
- **[`Display` from doc comments](serdes/derive_reference.md#display)**: Error enums print their doc lines, no hand-written `Display`.
- **[`#[cfg]` on enum variants](serdes/derive_reference.md#cfg-on-enum-variants)**: Compile a variant out; it keeps its number on the wire.
- **[Checked discriminants](serdes/derive_reference.md#ww_repr-repr-enums-only)**: Variant numbers must fit the chosen `ww_repr`, implicit ones count like in Rust.
- **[Generic field types](serdes/derive.md#type-mapping)**: `Wrapper<'i, T>` and types from `macro_rules` work as fields.
- **[Generic types](serdes/derive_reference.md#generics)**: Structs and enums with type and const parameters (`Timed<T>`, `Buf<const N: usize>`) and where clauses derive like any other type.
- **[`PhantomData` fields](serdes/derive_reference.md#generics)**: A zero-bit `PhantomData<U>` field carries a unit marker type (`Quantity<Volt>`) that needs no impls of its own.
- **[`#[default]` fields default in serde too](serdes/derive.md#evolving-types)**: A RON or JSON file saved before a field was added still reads, the field taking its default.
- **[`#[wire(W)]` fields](serdes/derive_reference.md#wirew-field-attribute)**: Keep a plain `i8` or `u64` in Rust and send it as `I4` or `UNib32`: range-checked on write, widened on read.
- **[`transparent` wrappers](serdes/derive_reference.md#transparent)**: A one-field struct (`Count(u64)`, `Volts<U>` with a `PhantomData` unit) is exactly its field on the wire, size class and `Series` columns included, so a new type costs no bytes.
- **[An enum's tag in a niche](serdes/derive_reference.md#niche-enums-only)**: `niche` lets an enum with a small field stay as small as the field.
- **[Narrow tag-only catch-all](serdes/derive_reference.md#no_future_version-future_version-name-enums-only)**: `#[future_version] Unknown(u16)` keeps a small enum small and still reads values from newer versions.
- **[Fields written as another type](serdes/derive_reference.md#wirew-field-attribute)**: `#[wire(W)]` keeps a small handle in Rust and writes the full value it stands for, also in a `Series`.
- **[Unit-typed fields cost nothing](serdes/code_size.md#transparent-fields)**: A `transparent` wrapper field (a `Fixed` number with its unit) compiles to the same code as its plain field, not only the same bytes.
- **[Growing writer](serdes/shrink_wrap.md)**: `BufWriterOwned` for hosts, when the size isn't known up front.
- **[Writing in place](serdes/showcase.md)**: Builders that write a value straight into the output buffer, and patching a field after the rest is written.
- **[Fixed layouts](serdes/layouts.md#fixed_layout-fields-at-fixed-offsets)**: Types with a fixed byte layout get fields at constant offsets, read and written without a parser, and a matching C header.
- **[Fixed head, sequential tail](serdes/layouts.md#head_tail-a-fixed-head-and-a-sequential-tail)**: `head_tail`, a format of its own for small firmware, with views that read fields on demand and a writer per enum variant.
- **[Checked C views](c.md#generated-readers)**: The generated C readers take a view made from the buffer and its length, so C code can't read a field past the end of the bytes it was given.
- **[`shrink_wrap` in C](c.md)**: A C99 reader and writer of the same format, no `malloc`.

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Format documented bit by bit](serdes/wire_format.md)**: How every type is laid out, and why.
- **[Small code on small CPUs](serdes/code_size.md)**: About 20 % less code on Cortex-M0 and RISC-V for the same bytes on the wire.
- **[Number access tuned per CPU](serdes/code_size.md)**: Multi-byte numbers through one function on RISC-V, inline on Arm, whichever is smaller.
- **[Size or speed of the bit paths](serdes/code_size.md#hot-paths-at-opt-level-z)**: Smaller by default at `opt-level = "z"`, the old speed or more with one `--cfg`.
- **[Leaner reader and writer plumbing](articles/flash_budget.md#sw-74-reader-and-writer-plumbing)**: The code each type repeats to make a reader or writer, take a latched error or read a size-prefixed value is shared, a few KB less in a firmware.
- **[Byte-wise `memcpy`](serdes/code_size.md#build-settings)**: Measured savings and downsides of a tiny `memcpy` for very small targets.
- **[Exact sizes of enums](serdes/size_assumptions.md#enums)**: `sized` enums report the discriminant plus one variant; variants of different sizes are a compile error.
- **[Strict bit reads](serdes/wire_format.md)**: Malformed data near the end of a buffer is an error, never a misread.
- **[Empty lists take no padding](serdes/wire_format.md#alignment)**: An empty byte list writes only its length, so the borrowed and owned types give the same bytes.
- **[Bytes to the end](types.md#special-types)**: `TailBytes` takes the rest of the buffer, written without a length; only allowed last.
- **[Size of the rest of a value](serdes/special_types.md#tailsizen-the-size-of-the-rest-of-a-value)**: `TailSize<N>`, filled in after writing, so a reader can skip a value or bound it.
- **[Lists written in place](serdes/special_types.md#refvecbuilder-lists-written-in-place)**: `RefVecBuilder` writes a list into the buffer one element at a time, from a loop or an iterator, with no slice and the same bytes.
- **[Series stores and size bounds](serdes/series.md#collecting-items-on-a-device-seriesstore)**: A batch of samples kept as one array per field until it is written as a `Series`, in half the RAM of the samples, and the most bytes the batch can take.
- **[Fast, small `Series` encoding on MCUs](serdes/series.md#cpu-and-flash-on-an-mcu)**: About 340 instructions per sample on a Cortex-M3, and codecs fixed at compile time that link only themselves: 15 KB of flash instead of 91 KB.
- **[`Series` straight from the store](serdes/series.md#from-the-store-column-by-column)**: A batch written column by column from the arrays it was collected in, the same bytes in half the instructions.
- **[Smaller enum decoders](serdes/derive.md)**: An enum mixing many plain variants with a few that carry data decodes in about half the flash on a small MCU, with the same bytes.
- **[Word-wide bit writer and reader](articles/series.md#the-bit-writer-and-reader-a-word-at-a-time)**: Bits written and read a word at a time, the same bytes: the observe poll of a 21-value FOC level in three quarters of the cycles on an STM32H725.
- **No `core::fmt::Debug` of `shrink_wrap::Error` on a device**: `Debug` is hand-written: variant names with `std`, a numeric code without, so a firmware that logs it through `defmt::Format` instead never pays for the names in flash. <span class="ww-status">hardware check open</span>
- **[One source copy of a length check](articles/flash_budget.md)**: `MaxLen<_, N>`'s bound check is one function shared by every `N` instead of a copy per bound, the same bytes on the wire and (measured) the same bytes in the binary.
- **[Sparse slots](serdes/special_types.md#optionanyvec-sparse-slots-of-any-types)**: `OptionAnyVec`, a list of optional values of any types where only the present ones take space.
- **[Lists of either-or values](serdes/special_types.md#eitheranyvec)**: `EitherAnyVec` holds one of two types per element, the flags packed by eight, e.g. a list of results.

<p class="ww-features__more" markdown>Details and what is planned: [Wire format (shrink_wrap)](serdes/features.md#wire-format-shrink_wrap-sw), [Derive macro (shrink_wrap)](serdes/features.md#derive-macro-shrink_wrap_derive-der), [Wire format](features.md#wire-format-shrink_wrap-sw) in the feature tracker.</p>

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
- **[Defaults through associated types](types.md#generic-types-in-an-api)**: `Niched<V, N>` with its storage left to `<V as RawValue>::Raw`: parameter defaults that name another parameter's associated type work in an API.
- **[Type aliases](types.md#type-aliases)**: `type Volt = Quantity<VoltUnit>` and generic aliases anywhere in an API; tools show what they name.
- **[Dependencies from anywhere](api/traits.md#reusing-traits-from-other-crates)**: An API crate can take traits and types from git and crates.io crates, not only from a path next to it.
- **[Types from other crates](types.md#types-from-crates-that-arent-api-crates)**: An API can use types of a crate that isn't built on WireWeaver, also one with `shrink_wrap` optional behind a feature, or one the server only reaches through a re-export.
- **[Types in modules](types.md#types-in-modules)**: API types can live in any `pub mod` or be re-exported under another name; generated code finds them.
- **[Self-referential types](types.md#self-referential-types-refbox)**: Trees and expressions (`RefBox`, a list of the type itself) anywhere in an API, also in the dynamic client and Python.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Optional items](api/optional_items.md)**: Leave out what a board doesn't have, with no flash spent, and clients know before they call.
- **[Optional traits](api/optional_items.md#optional-traits)**: Leave a whole block of the API out of a build, e.g. behind a Cargo feature, and clients see it is not there.
- **[Error lists](api/methods.md#error-lists)**: Which errors each method can return, shown in docs and tools and checked in tests.
- **[Effects](api/metadata.md#effect-what-an-item-does)**: Every method, writable property and sink says what it does (`read`, `persist`, `actuate`, ...): GUIs, agents and the hub know what is safe to call.
- **[Stream functions](api/streams.md#stream-functions)**: Open a stream with typed arguments and a result, like a method call.
- **[Observing properties](api/observing.md)**: Ask a device to send changes on change or periodically, within a byte budget.
- **[Observing every sample](api/observing.md#every-sample-batched)**: A fast property's every sample, each at its time, in compressed batches; typed and dynamic clients, the CLI and Python.
- **[One clock per observed level](api/observing.md#every-sample-batched)**: The samples of a level's properties share one time column per update: about a third fewer bytes and half the encoding CPU.
- **[Observing elements of resource arrays](api/observing.md#levels-under-resource-arrays)**: Observe a property of one element of an array (`players[2].state`): updates for that element only, and a removed element ends its observers.
- **[Running totals](api/totals.md)**: A pointer or scroll wheel over UDP: each sink write carries the sum so far, so a lost write loses nothing and nothing waits.
- **[The API crate prelude `ww_api`](api/folder_structure.md)**: An API crate depends on `ww_api`, not the runtime: the macros, `shrink_wrap` and the common types in one prelude, checked by the layer lint.
- **[Keyed resource arrays](api/arrays.md)**: Address the elements of an array by a key (`keyed!(flow by FlowKey)`) instead of the index the device gave them.
- **[Observe cells](api/observing.md#cells-sampling-from-an-interrupt)**: Sample a level from an interrupt in place, value by value into a ring of atomics: a quarter of the cycles and a tenth of the stack of `sample()` at opt-level z, the same wire.
- **[A cheaper observe poll](articles/series.md#cpu-and-flash-on-a-microcontroller)**: The poll that sends a level's samples spends its time on the codec: half the cycles at opt-level 3, a third at z, the same bytes.
- **[Batched streams](api/batching.md)**: A stream opts into sending samples many per event as a compressed `Series` instead of one at a time, for plain, counted and reliable streams; typed and dynamic clients, the CLI and Python.
- **[A time point per batched stream](api/batching.md)**: `batched(time = DeviceMillis<u32>)` gives a quiet or slow stream a time that wraps later, or another resolution.
- **[Compressed streams](api/stream_compression.md)**: A counted stream of text (a UART, a log) goes LZ4-compressed with history across events, 3-5x, the device says which codec, the host decodes it or asks for the items as they are. <span class="ww-status">hardware check open</span>
- **[Size analysis](types.md#worst-case-size-at-compile-time)**: Worst-case request and reply sizes as constants, checked at compile time; properties and streams are next. <span class="ww-status">in progress</span>
- **[API docs in rustdoc](api/traits.md#in-rustdoc)**: `#[ww_trait]` also leaves a plain Rust trait, so methods, properties and streams show up on docs.rs. <span class="ww-status">in progress</span>
- **Code generation from `build.rs`**: `gen_server` / `gen_client` as an alternative to the macros.
- **[Servers on std with owned types](transport/hub.md#servers-on-std-no_alloc-false)**: A server running on a PC returns `String`, `Vec` and owned types from its handlers, the same bytes on the wire as a firmware's.
- **[Generated C](c.md)**: The C runtime is there; generated C for an API's types and server is next. <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **Builders for events and errors**: Written straight into the output buffer.
- **[Dispatch on index slices](api/addressing.md)**: Requests are routed by `match` on their path, the base of multi requests.
- **[Names and paths that don't collide](api/folder_structure.md)**: Generated names carry the whole trait chain; type paths come from Cargo.
- **[Counted and reliable streams](api/stream_reliability.md)**: Exact counts of lost items; acknowledgements, resends and resume after a reconnect. <span class="ww-status">in progress</span>
- **[Credit sinks](api/credit_sinks.md)**: Flow control for data sent to a device, so its buffer never overflows, and lossy links resume where they stopped.
- **[Resend and resume inside a batch](api/stream_protocol.md#positions-inside-an-event)**: A resend or a resume from the middle of a multi-unit item gets only the units you don't have.
- **[Credit sinks without a size limit](api/stream_protocol.md#positions-wrap-at-256-mib)**: Sessions of any length, positions at most 4 bytes on the wire.
- **[Credit sinks that resume](api/credit_sinks.md)**: A sink (UART tx, a DFU plan) continues after a reconnect or a host restart, on the same acknowledgements as reliable streams. <span class="ww-status">hardware check open</span>
- **[Reconnect](api/credit_sinks.md#resume)**: `Commander::reconnect()` connects again after `disconnect().keep_streams()` and resumes streams, sinks and observers. <span class="ww-status">in progress</span>
- **[Resume checked on hardware](api/credit_sinks.md#resume)**: A UART sink on a real board put 1 MB on the line exactly once through link drops and a host restart.
- **[Trait paths](api/addressing.md)**: Call a shared trait wherever it sits in a device's API. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Wire format](features.md#wire-format-shrink_wrap-sw), [Server / device side](features.md#server-device-side-srv), [Documentation](features.md#documentation-doc), [Examples](features.md#examples-ex), [Testing, CI and performance](features.md#testing-ci-and-performance-test) in the feature tracker.</p>

## Device side

The server is the code on the device that answers requests. It is generated from the trait, needs neither `std` nor an
allocator and does no IO itself, so it runs on any MCU and RTOS over the transport you choose.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[`no_std`, no allocator](api/methods.md#server-side)**: The generated server works on the smallest MCUs and does no IO itself.
- **[Your event loop](transport/overview.md)**: An async `Server` with `wait()` / `handle()` inside your own loop, or a blocking one.
- **[Media](transport/overview.md)**: USB (embassy), RTT, WebSocket and UDP, the last two also on `embassy-net`.
- **[Several hosts on one UDP port](transport/udp.md#several-hosts-on-one-port)**: Each host gets a link of its own on one port: a fixed set of slots on a device, a connection per host on a computer.
- **[Device descriptors](transport/usb.md)**: Serial number, API name, version and a user label, visible before connecting.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Server of a keyed array](api/arrays.md#keyed-arrays)**: `ww api scaffold` prints two handlers per `keyed!`, the generated code answers the rest.
- **[Deferred replies everywhere](api/methods.md#deferred)**: Answer a call later, also on nested trait levels.
- **[Replies by priority](transport/ww_framer.md#latency-and-priority)**: A reply to an urgent request leaves at once, also a deferred one sent later from your main loop; `Sink::flush` sends what waits now. <span class="ww-status">hardware check open</span>
- **[Errors that say where](api/errors.md#where-on-the-device-an-error-came-from)**: A device's error names the site that raised it, by a number that is the same in every firmware.
- **[Who may call](api/host_server.md#who-may-call)**: A host service lets in the same user, a group or everyone on its local socket, with matching file modes, and asks a hook of yours before each request.
- **[Deadline and cancel](api/host_server.md#deadline-and-cancel)**: A request carries the client's timeout and trace: a host server drops it when the time runs out or the client gives up, queued or running, and handlers read both from `cx.call()`. <span class="ww-status">in progress</span>
- **[Who asked](api/local_trait.md#who-asked-callcontext)**: Every call carries a small `CallContext`: the peer as the transport saw it, priority, deadline and trace, the same for a request a server handles and a call in the same process.
- **[Host server](api/host_server.md)**: One API served to many clients at once on any async runtime, in-process or on a local socket (also under systemd socket activation): a backend per connection, calls side by side, streams that flow while calls run, limits that push back, graceful shutdown.
- **[Hosts on UDP](api/host_server.md#hosts-on-udp)**: A host server takes a connection per host of one UDP socket, on any thread of a multi-threaded runtime.
- **[The hub on the host server](transport/hub.md#its-api)**: The ww hub serves its own API on `wire_weaver_server`: its streams flow while calls run, and its clients learn why it went away.
- **[On a calloop loop](api/host_server.md#on-a-calloop-loop)**: A host server runs on a calloop event loop, WebSocket and UDP clients on one port.
- **[Sync handlers in an async server](api/handlers.md)**: Handlers that never wait are plain `fn`s, only the ones that await are `async`: less flash.
- **[Shared handlers](api/handlers.md#shared-and-exclusive-handlers)**: Calls and reads of the items you mark shared take `&self`, so a host server can run them side by side; the rest stays `&mut self`.
- **[A server from the local trait](api/local_trait.md#serving-a-local-implementation)**: `serve_trait` generates every handler of a host server from one implementation of the API's local trait: the same code serves calls in the same process and remote clients.
- **[Authorized local calls](api/local_trait.md#authorization-in-the-same-process)**: The hook that guards a host server guards calls in the same process too, and can hold a call until a person approves it.
- **[Local trait without `Send`](api/local_trait.md#without-send)**: The local trait in a form for one thread, state in `Rc` and `RefCell`: implemented by your struct and the client, served by `serve_trait`.
- **[When a connection ends](api/host_server.md#when-a-connection-ends)**: A host server's backend gets a last step when its client goes, to release what the client held.
- **[Listeners that stop alone](api/host_server.md#moving-to-another-address)**: Stop one listener of a host server while its connections go on, to move to another address.
- **[Observed properties](api/observing.md#the-device-side)**: The device sends property changes within the budget each host asked for.
- **[Event-driven observe](api/observing.md#the-server-loop)**: The device sleeps until an observed property is due, and wakes at once when one changes.
- **[ECC-safe STM32 flash](api/ecc_flash.md)**: Reads a flash word broken by a power cut as zeros (stores) or as an ECC error (bootloaders) instead of crashing. <span class="ww-status">hardware check open</span>
- **[I2C and SPI slave engine](std_library/bus_slave.md)**: Register map, answer queue, clock stretching and events of a slave, wired to your peripheral's interrupts.
- **Streaming LZ4**: `ww_lz4` compresses a byte stream chunk by chunk on the device, with history across chunks: shell text 3-5x smaller in 512 B chunks, standard LZ4 blocks any LZ4 library decodes.
- **[A host write reaches observers](api/observing.md#the-device-side)**: An observed `get_set` property written by a host is sent to its on-change observers without the setter marking it: the generated server marks it after a successful set.
- **[Leave out multi requests and counted streams](serdes/code_size.md#request-kinds-a-firmware-leaves-out-multi-and-streams)**: A firmware that doesn't serve them builds `ww_client_server` without `multi` and `streams`: 1.6 KB less flash and about 12 % fewer instructions per request at `opt-level = "z"`, and it answers such requests as unsupported.
- **[Observe costs nothing unused](articles/flash_budget.md#observe-left-out-srv-54)**: A firmware that observes nothing links no observe code: on a Cortex-M0+ a one-method server went from 26 KB to 14.5 KB of flash.
- **[No observe encoder without samples](api/observing.md#a-firmware-that-takes-no-samples)**: `observe_samples = false` in `ww_codegen!`: a firmware that takes no samples of its `+batched` properties links no batcher, outbox or `Series` encoder, about 20 KB on a Cortex-M0+, with the same API and bytes for hosts; `Samples` is refused. <span class="ww-status">in progress</span>
- **[Dispatcher arms share their replies](articles/flash_budget.md#dispatcher-arms-only-what-differs-srv-51)**: Each resource of a firmware's server costs only its decode and handler call, the replies are shared: on a Cortex-M0+ one more resource went from 238 B to 86 B of flash.
- **[Dispatcher errors written in place](articles/flash_budget.md#errors-written-in-place-srv-52)**: A request's error is written where it happens instead of travelling up the levels as a `Result`: smaller firmware and fewer instructions per request.
- **[Reply paths shared by every type](articles/flash_budget.md#reply-paths-without-a-copy-per-type-srv-55)**: A firmware has one copy of each reply path whatever the types of its resources: Portwright's firmware 17 KB smaller.
- **[Global trait paths without names in flash](api/addressing.md#who-resolves-a-global-path)**: A device with introspection carries no global trait lookup, the client resolves global paths from the introspection data; without introspection the device finds a trait by a 32-bit hash of its crate and name.
- **[Claims, pins and labels](api/metadata.md)**: Mark a bus or port `#[claim(..)]`, the pins it `#[shares]` and a `#[label]` a person recognises: in introspection for the hub, `ww` and GUIs, so one program can have a resource to itself through the hub while others keep reading.

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **Split send and receive**: Independent TX and RX paths for every transport; handlers called straight from the receive loop.
- **[Stream stores](api/stream_stores.md)**: Data a host hasn't acknowledged yet, kept in RAM or in a power-cut safe log on SD card or flash.
- **[Logs that survive a crash](api/stream_stores.md)**: A store in RAM that outlives a reset, for what happened just before a fault.
- **[Smaller observe code](api/observing.md#the-device-side)**: A firmware carries the observe logic once, not once per trait level: about 15 KiB less flash on a big API.

<p class="ww-features__more" markdown>Details and what is planned: [Server / device side](features.md#server-device-side-srv), [Server / device side (ww_stdlib)](std_library/stdlib_features.md#server-device-side-srv) in the feature tracker.</p>

## Host side

On the PC, a generated Rust client calls a device like a local library. A dynamic client does the same for any device
from the description the device sends of itself; the `ww` tool and Python are built on it.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Rust client](api/methods.md#client-side)**: Async, blocking or promise-style calls on a typed client.
- **[One `?` per call](api/methods.md#client-side)**: Device errors and link errors in one `Result`.
- **[Transports](transport/overview.md)**: USB, RTT through a debug probe, WebSocket, UDP, and in-process for tests.
- **[Picking a device](transport/overview.md#connecting)**: By serial number, label or path, with clear errors when none is found.
- **[Compatibility check on connect](evolution/rules.md)**: The client compares its API with the device's and tells what doesn't match.
- **[Hello wire matches the bundle](evolution/versions.md#the-wire-version)**: A device whose hello and its own introspection bundle disagree on its wire version is refused, naming both, instead of trusting whichever claim came first.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Dynamic client](std_library/ww_self.md#dynamic-access-helpers)**: Talk to an API known only at runtime.
- **[Cached API descriptions](std_library/ww_self.md)**: A device's description is downloaded once, then found by its hash.
- **[Multi requests](api/multi_requests.md)**: Read many properties or call a method on many array elements in one request.
- **[Fallible methods in multi requests](api/multi_requests.md#methods-returning-result)**: Batch methods returning `Result` (an I2C write and read) into one round trip, each answering what its own call would.
- **[Same method twice in a multi call](api/multi_requests.md#several-resources-at-the-same-level)**: Two writes over the same I2C bus (or any repeated call) go into one batch, answers in order.
- **[Observing properties](api/observing.md#the-host-side)**: Typed subscriptions and a cache of the latest values, shared by everyone watching.
- **[`Series` without generated code](serdes/series.md#introspection-and-dynamic-clients)**: `ww`, Python and any dynamic client read and write a `Series<T>` from introspection alone, Python reads it as numpy columns.
- **[Quantities without generated code](std_library/ww_si.md#in-ww-python-and-dynamic-clients)**: `ww` and Python show a `ww_si` value with its unit (`3300 mV`), Python as a quantity object with `to_si()`.
- **[Unit references through the model](std_library/ww_si.md#in-ww-python-and-dynamic-clients)**: A dynamic client resolves a value's unit references through the API's model and writes the API's units by reference.
- **[Exact units without generated code](std_library/exact_units.md#in-ww-python-and-dynamic-clients)**: `ww` and Python show a `Fixed` value as the device stores it (`1730000000000000 µs`, `4 KiB`) and write `5 ms` into it exactly, or refuse with the type named instead of rounding.
- **[Points without generated code](std_library/points.md#in-ww-python-and-dynamic-clients)**: `ww` and Python show a `Point` (a temperature in °C, a heading) in its own origin, not its `Fixed`'s raw unit.
- **[Typed times of batched items](api/batching.md#host-side)**: A timed batched stream's items carry their full device time as a typed point (`DeviceMicros<u64>`), not a `Second`.
- **[Client of a keyed array](api/arrays.md#keyed-arrays-on-the-client)**: `device.flow_by(&key)` gives an element's client by its key; the index is asked once and kept in a key cache.
- **[Python module as a library and a wheel](python.md#installation)**: `wire_weaver_py` is a library crate; the wheel `wire_weaver_wheel` (vhrdtech/ww_tools) is a thin `cdylib` holding `pyproject.toml`, the Python package and the tests.
- **[Link setup timeout](cli.md)**: Connecting over a slow link (a debug probe) waits seconds, a fast one fails quickly, and you can set the time.
- **[Large messages](api/large_messages.md)**: Set the limit in `ClientConfig`; both sides keep to the smaller one, an over-limit call fails at once naming both numbers, `no_alloc = false` servers reply past their scratch buffer, and results that grow with the data go as a stream of rows or in pages.
- **[Local trait](api/local_trait.md)**: The API as an async Rust trait that the client and your own struct both implement, so one call site works in process (a direct call) or remote. <span class="ww-status">in progress</span>
- **[Local trait for levels and arrays](api/local_trait.md#nested-levels)**: Every nested level gets its own trait, reached by an accessor; resource and keyed arrays take the element's index or key, locally, on the client and through `serve_trait`. <span class="ww-status">in progress</span>
- **[Calls that work on any device](std_library/device_contract.md)**: Shared traits called whatever a device's own API version. <span class="ww-status">in progress</span>
- **[Calls fail fast while disconnected](transport/hub.md#reconnect-after-a-hub-restart-clt-40)**: A call made while the link is down fails at once with `Disconnected` instead of waiting for a reconnect.
- **[Urgent requests](transport/ww_framer.md#latency-and-priority)**: `.urgent()` (or `.priority(p)`) on a call, read or write sends it and its answer at once instead of waiting for the link's accumulation window; everything else shares frames. <span class="ww-status">hardware check open</span>
- **[Windows kept to the microsecond](transport/ww_framer.md#latency-and-priority)**: A link that batches messages for 1 ms waits 1 ms, not up to 2 for the PC's timer tick. <span class="ww-status">hardware check open</span>
- **[Who holds a device](transport/hub.md#who-holds-a-device-clt-38)**: A busy error names the program already holding a USB interface or RTT probe, pid and since when; a crash releases it right away.
- **[Hub routing in connect](transport/hub.md#reaching-devices-through-the-hub-clt-39)**: A device the ww hub holds, or one opened again and again from a shell, is reached through the hub, which a program starts by itself when needed; `WW_HUB` says when. <span class="ww-status">in progress</span>
- **[Where a connect's time goes](transport/overview.md#where-a-connects-time-goes)**: A connect reports each step as a `tracing` span, and `just connect-bench` times them for many clients at once or a board on USB, cold and warm. <span class="ww-status">in progress</span>
- **[Reconnect onto another API revision](transport/keep_trying.md#another-revision-of-the-devices-api)**: A reconnect to other firmware on the same wire line learns its API again and resumes what still fits.
- **[Keep-trying connections](transport/keep_trying.md)**: A client that exists at once, connects when the device comes up, reconnects whenever the link goes and resumes its observers and streams, racing its addresses; `retry_now()` hurries it. <span class="ww-status">in progress</span>
- **[Addresses raced](transport/keep_trying.md#what-it-does)**: A device reachable at several addresses (LAN and VPN) is connected at whichever answers first, so an address that is gone costs nothing.
- **[Back after a hub restart](transport/hub.md#reconnect-after-a-hub-restart-clt-40)**: A program reaching its device through the ww hub reconnects by itself when the hub crashes, is killed or hands over, and its counted and reliable streams go on; the app hears about the gap. <span class="ww-status">hardware check open</span>
- **[ww plugins](cli_reference.md#plugins)**: `ww NAME` runs `ww-NAME` from PATH like cargo does, with your device selection; `ww dfu flash` is the `ww-chunky` plugin.
- **[Clocks steered by every program](std_library/ww_timebase.md#steering-in-every-direct-connection)**: A program connected to a board directly keeps the board's clock on the PC's UTC by itself, and leaves it to the ww hub when it goes through one. <span class="ww-status">hardware check open</span>
- **[Polite handover to the hub](transport/hub.md#polite-handover-from-a-direct-holder-clt-43)**: A command-line tool or notebook holding a device lets the hub take it when another program asks, and goes on through the hub after a few milliseconds; a GUI says no and is named in the other's error. <span class="ww-status">hardware check open</span>
- **[One lock file per device](transport/hub.md#device-keys)**: A USB device with a crafted serial number can't make another device look held: every device key gets a lock file of its own.
- **[Live link handover on Linux](transport/hub.md#live-link-handover-clt-44)**: A program holding a USB device hands its open link to the hub without a reconnect: its calls in flight are answered, its streams go on, the device notices nothing. <span class="ww-status">hardware check open</span>
- **[Who connects](transport/hub.md)**: Every client tells the device its program, user, host, pid and kind in link setup; a device can log it, a hub or a lock file shows it.
- **[STM32 ROM DFU](transport/follow_reboot.md#stm32-system-bootloader-rom-dfu)**: Flash an STM32 over USB through its built-in bootloader (`ww dfu stm32 flash`), also straight after the app rebooted into it.
- **[Following a reboot](transport/follow_reboot.md)**: Stay with a device while it reboots into its updater (RP2040 / RP2350 boot ROM so far). <span class="ww-status">in progress</span>
- **[Updating a chunky device](cli_reference.md#ww-dfu-flash)**: `ww dfu flash app.elf`: only what changed goes over the wire, then a trial boot that the new firmware confirms or the device reverts. <span class="ww-status">in progress</span>

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
- **[USB VID:PID](transport/usb.md)**: Set your own assigned VID:PID on the device side instead of the shared test ID.
- **[Announced on the LAN](transport/websocket.md#announcing-on-the-lan)**: A WebSocket server announces itself over DNS-SD as `_wireweaver._tcp`, with its API and wire version, so it is found without a typed address.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Framer](transport/ww_framer.md)**: Packs messages into packets with 1 byte of overhead, CRC and recovery from lost frames.
- **[Link layer](transport/ww_link.md)**: Link setup, device info and protocol version exchange.
- **[Frame counter and setup token](transport/ww_link.md#setup-token)**: Nothing of a previous session is taken for the new one after a reconnect; UDP datagrams carry a frame counter and a CRC-32 on every message, USB packets nothing they don't need.
- **[One CRC crate](transport/ww_framer.md#checksum-and-tail)**: The framer, link, block, settings and stream crates share one CRC table per algorithm, its size picked once per firmware.
- **[Simulated UDP](transport/udp.md#simulated-udp-in-tests)**: UDP datagrams without the kernel for tests: the paused clock decides which messages share a datagram, so a transcript has the same bytes on every run.
- **[Simulated WebSocket](transport/websocket.md#simulated-websocket-in-tests)**: WebSocket over an in-memory pipe for tests: the paused clock decides the order of both ends' frames, so a transcript is the same on every run.
- **[USB vendor descriptor constants](transport/usb_internals.md#device-layout)**: The USB class, subclass and vendor-specific descriptor bytes a WireWeaver USB device writes live in `ww_link`, shared by every USB stack so they write the same bytes.
- **[The ww hub](transport/hub.md#the-hub-process-srv-28)**: One small process per user that knows the devices, who holds them and which programs are connected, with its own API. <span class="ww-status">hardware check open</span>
- **[`ww hub`](cli_reference.md#ww-hub)**: See and steer the hub from the shell: its status and who started it, who holds each of your devices (the hub, another ww program, or any program on Linux), keep it up for a bench, stop it; `ww list` shows what it holds. <span class="ww-status">in progress</span>
- **[The hub from Python](transport/hub.md#python-and-notebooks-tool-18)**: `python -m wire_weaver.hub` starts it from the wheel, `connect()` reaches a device through it, and `wire_weaver.hub()` talks to the hub itself; a notebook kernel restart reconnects in no time with the device still open.
- **[Clocks steered by the hub](transport/hub.md#clock-sync-in-the-hub-srv-31)**: Boards with a shared clock stay on the PC's UTC between short commands: the hub steers them, `ww sync` shows how, and a board the PC knows is steered right after it opens.
- **[Many programs, one device](transport/hub.md#device-tunnels-srv-29)**: Several programs use one device at once through the hub, each with its own calls, streams and observed properties. <span class="ww-status">hardware check open</span>
- **[Recording through the hub](transport/hub.md#captures-recording-through-the-hub-srv-64)**: Programs ask the hub to record a device's link, share one capture, watch it live at their own pace and save it to a file; nothing waits on a slow reader. <span class="ww-status">in progress</span>
- **[Hub version skew](transport/hub.md#version-skew-and-handover-srv-30)**: Programs built with different WireWeaver releases share one hub; a newer program replaces an older hub without losing anyone, and a hub that can't serve a program says which versions and what to do. <span class="ww-status">hardware check open</span>
- **[The whole device, or lent out](transport/hub.md#the-whole-device-exclusive-access-and-lending-srv-32)**: A firmware update has the device to itself while other programs keep reading, and `ww hub lend` hands it to probe-rs or dfu-util for one command and takes it back. <span class="ww-status">hardware check open</span>
- **[Byte streams](transport/byte_streams.md)**: Client and server over TCP, a command's stdin and stdout (ssh) or any stream you open, with several APIs on one listener, also over WebSocket.
- **[Local sockets](transport/hub.md)**: A device served by another process on the same PC (a simulator, later the ww hub), reachable by your own user only. <span class="ww-status">hardware check open</span>
- **[Link tap](transport/tap.md)**: Every frame, message and link event of a link recorded and decoded: which packet a request went out in, what a lost packet took with it.
- **[USB throughput on small MCUs](transport/usb_internals.md)**: Measured and tuned on an STM32G0 so far. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Transports and protocols](features.md#transports-and-protocols-tr), [Checksums (ww_base)](std_library/features.md#checksums-ww_crc-tr), [Server / device side](features.md#server-device-side-srv), [Evolution, versioning and compatibility](features.md#evolution-versioning-and-compatibility-evo) in the feature tracker.</p>

## Introspection

A device can send a description of its own API, so tools and scripts work with it without its source code.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Devices describe themselves](std_library/ww_self.md#the-api-model)**: The API model, sent by the device on request.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Bit-sized numbers in the model](std_library/ww_self.md)**: `U7`, `I12`, nibbles and the like, read and written by the dynamic client and Python.
- **[Units in the model](std_library/units_in_the_model.md)**: A device's API carries the definition of every unit, scale and origin it uses, also of your own crate's, so `ww`, the dynamic client and Python convert and show them without a built-in table.
- **[Compressed introspection data](api/introspection.md#compressed-introspection-data)**: `introspect_compression = "lz4"` keeps a device's API description compressed in flash, the client decompresses it: 4.3 KB less on rePort.
- **[Identity of a renamed resource](evolution/rules.md#moving-a-resource-to-another-place)**: A resource renamed with `#[moved_from]` keeps its first name in the device's API description, so recordings follow it across the rename.
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
- **[A dependency changed in place](evolution/inside_the_checker.md#a-dependency-changed-without-a-version-bump-evo-67)**: A same-bytes change of a dependency's type under the same, unpublished version is compared by its two definitions, not reported as a break in every crate that embeds it.
- **[Superseded definitions](evolution/inside_the_checker.md#superseded-definitions-evo-68)**: A snapshot refreshed in place keeps the definitions it replaced, so frozen API lines and older devices that name them by signature are still compared by the evolution rules.
- **[Old-format snapshots, refreshed pins](evolution/inside_the_checker.md#a-snapshot-saved-by-an-older-wire_weaver-evo-70)**: A snapshot saved by an older wire_weaver still has a dependency's changed definitions compared, and `ww api refresh-pins` keeps a frozen fixture's pins in line with the source.
- **[Finding snapshots](evolution/snapshots.md)**: A host finds the snapshot of a dependency's crate version wherever it is (embedded in `ww`, the cache, Cargo's sources, crates.io or the crate's repository), and shows a value it can't decode as opaque instead of failing.
- **[Snapshot model format](evolution/snapshots.md#model-format)**: A snapshot file names the format of the API model it was saved in; a file of another format is refused with the step that re-saves it.
- **[Evolution of keyed arrays](evolution/rules.md#keyed-arrays)**: `ww api check` knows `keyed!`: a new one is compatible, a changed key type is checked both ways, and across a break it follows the array it keys.
- **[Moving definitions into modules](evolution/checker_tool.md)**: Moving types or a trait from `lib.rs` into a `pub mod`, re-exported at the same paths, or writing them in another order, is no change for `ww api check` and `ww api save`: a crate snapshot depends on the API only.
- **[Shape signature and docs signature](evolution/inside_the_checker.md#doc-comments-of-a-dependency-evo-69)**: A dependency's doc fix changes no identity: devices, snapshots and the checker match definitions by a shape signature without docs, and a docs-only change is a note or "documentation updated".
- **[The device declares a stream's codec](api/stream_compression.md)**: A compressed stream's device says which codec its events carry and the host decodes it; a host that can't asks for the items as they are.
- **[One cause, not many breaks](evolution/checker_tool.md#several-broken-items-one-cause-evo-21)**: Several broken traits or types caused by the one dependency that changed are reported together, naming it once.
- **[Arguments that evolve](api/methods.md#arguments-that-evolve)**: Add trailing arguments with a default, or turn arguments into one `#[flatten]` struct, without breaking the wire.
- **[Frozen hello](transport/ww_link.md#the-frozen-hello)**: Link setup opens with a few fixed bytes that name each layer's generation, so a host and a device that can't talk say which side to update instead of timing out.
- **[Link setup decides on the wire line](evolution/versions.md#the-wire-version)**: A device and a host of one API connect when they are on the same wire line, whatever their crate versions, so a Rust-only break no longer cuts old devices off; another line is refused naming both.
- **[Wire version of its own](evolution/versions.md#the-wire-version)**: `[package.metadata.wire_weaver] wire = "w2.3"` in the API crate's `Cargo.toml` says which devices and hosts talk, apart from the crate version, and `#[since = "w2.1"]` names the revision that added an item.
- **[Wire versions of shared crates](evolution/crate_layers.md#layer-2-ww_stdlib)**: A `ww_stdlib` crate declares its own wire version in `Cargo.toml`: its `#[since]` markers name its revisions, and a Rust-only break of the crate leaves the APIs using it on their wire line.
- **[Both bumps from one check](evolution/checker_tool.md#two-bumps-the-wire-and-the-crate)**: `ww api check` says which wire version and which crate version a change needs, and fails when either is not bumped enough.
- **[Generations from the wire line](transport/ww_link.md#the-frozen-hello)**: The generation bytes of the frozen hello are the wire lines of their crates, declared in `Cargo.toml` like any other wire version, so a layer's break is bumped in one place and the evolution checker sees it.
- **[Wire version in Cargo.toml](evolution/versions.md#the-wire-version)**: Every crate declares its wire version one way, in its `Cargo.toml` next to its crate version; the attribute form is left for test crates that stand for several lines.
- **[A host for several wire lines](evolution/multi_line_host.md)**: One host driver carries a generated client per wire line of an API and talks to devices of any of them through one facade, picking the client of the device's line in a single link setup; scripts reach every line through the dynamic client and Python, writing quantities exactly into each line's types.
- **[Several wire lines in one crate](evolution/versions.md#several-lines-in-one-crate)**: One API crate holds every wire line still in use, a root per module with its own wire version, and the types the lines share are one Rust type.
- **[The multi-line host on one crate](evolution/multi_line_host.md#every-line-in-one-api-crate)**: The worked example of a host for three wire lines uses one API crate: the types the lines share are one Rust type converted once, one client serves a shared sub-trait, firmware built from the older crates still connects, and what each line costs is measured.
- **[One client for a shared sub-trait](evolution/multi_line_host.md#shared-sub-traits)**: The lines of one crate that reach the same sub-trait (`common::Channel`, `common::Indication`) by path return one generated client type for it, checked at compile time to be the trait each line has.
- **[Fallible client pick](evolution/multi_line_host.md#connecting-once-on-either-line)**: A facade of several wire lines returns an error for a line it has no client of, instead of panicking, though link setup already refused every other line.
- **[A facade without boilerplate](evolution/multi_line_host.md#the-facade)**: A host driver for several wire lines lists its operations once and gets an enum over the line clients with a method per operation; each line writes only the bodies, and an operation a line lacks is refused naming what has it.
- **[Reconnecting onto another line](evolution/multi_line_host.md#reconnecting-onto-another-line)**: After a DFU to new firmware, a reconnect picks the client of whichever wire line the device is on now instead of refusing it; a client that can't (one line only) ends the session instead, naming both lines.
- **[Snapshots per API root](evolution/checker_tool.md#several-api-roots-in-one-crate)**: A crate with several lines saves a snapshot per root when it changed, checked against that root's previous one, and never rewrites a released one.
- **[w0, the design line](evolution/versions.md#w0-the-design-line)**: A new API starts on `w0.0`, where breaks need no new line, no snapshots are kept and a client checks the device's introspection data instead of trusting a revision; the first release moves to `w1.0`.
- **[A shared crate cache that sees edits](evolution/checker_tool.md#checking-each-root)**: `ww api check` and the evolution test keep what they loaded only while the source files are unchanged, so a repeated check of an unchanged crate loads nothing again and an edit is always seen.
- **[API loads kept across proc-macro invocations](api/overview.md)**: A crate with several `ww_codegen!` calls over one API (a server and a client together) reads that API once per compile, and again only after a file it read changed, also in rust-analyzer.
- **[The checker per API root](evolution/checker_tool.md#checking-each-root)**: `ww api check` checks each line of a crate against its own latest snapshot, names a changed shared type with every line it changed on, and treats a removed line as a breaking crate bump.
- **[Readable findings on a new line](evolution/checker_tool.md#what-is-checked)**: A line break is reported item by item: no noise from unchanged sub-traits, every difference of a changed field, the right version labels.
- **[`#[moved_from]`](evolution/rules.md#moving-a-resource-to-another-place)**: Across a wire break the checker matches resources by path and by `#[moved_from]` on their new place, verifies a moved resource is exactly the same and reports a move; a resource of the same name elsewhere is only proposed.
- **[Wire versions and generations shown](std_library/device_contract.md#ww-info)**: `ww info` and the Python client show a device's wire version and the hello generations a connection agreed on, next to its crate version.
- **[Unit definitions with an identity](std_library/units_in_the_model.md#identity)**: The API model holds each unit's whole definition with the crate and marker that declare it, so two crates' units of one name stay apart.
- **[Unit references on the wire](std_library/ww_si.md#units-known-only-at-run-time)**: A run-time unit names a unit the API lists by its crate and marker, a few bytes instead of its whole definition.
- **[Checker knows the catch-all](evolution/rules.md#unsized-types)**: Appending a variant to an enum with the `Unknown` catch-all passes the evolution check as compatible.
- **[Evolution check at every commit](evolution/checker_tool.md#checking-at-every-commit)**: A test of your API crate fails when the API changed without the version bump it needs.
- **[Current version's snapshot required](evolution/checker_tool.md#checking-at-every-commit)**: The API crate's test fails until the version it is at has a saved snapshot, and saves it for you.
- **[Priority in every request](transport/ww_framer.md#latency-and-priority)**: Requests carry a priority; hosts and devices built before it don't talk to ones after.
- **[Priority as on a CAN bus](transport/ww_framer.md#latency-and-priority)**: A request's priority has 8 levels in 3 bits, as CAN and Cyphal carry in a frame's ID, with one bit reserved beside it; the upper four are urgent.
- **[Upgrades don't touch your snapshots](evolution/checker_tool.md#the-current-versions-snapshot)**: A newer wire_weaver compares an older snapshot only by what it recorded: no version bump or re-save just for upgrading.
- **[One encoding per dimension](std_library/ww_si.md#units-known-only-at-run-time)**: A unit's dimension on the wire has exactly one form: distinguished bases sorted, out-of-order, repeated or zero ones refused, so two devices' `Sample/Frame` compare equal byte for byte.
- **[Units and bounds in the checker](evolution/rules.md#transparent-wrappers-units-and-scales)**: Wrapping a plain number in a unit type passes as the same bytes; changing its unit, prefix or bound is caught as breaking.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[API identity](api/versioning.md)**: Every API and type has a signature; shared traits are referred to by hash.
- **[API snapshots](evolution/crate_layers.md)**: Descriptions of the standard library are left out of what a device sends and filled in by the host.
- **[Definitions from another revision](std_library/ww_self.md#the-definition-cache)**: A device built from an unpublished revision still connects, with a warning.
- **[Units compared by meaning](evolution/units_by_meaning.md)**: `Milli` → `Pow10<-3>` passes as the same value; lbf·s → N·s is caught as breaking with the factor, as are another zero of a point or a wrapping number on one side.

<p class="ww-features__more" markdown>Details and what is planned: [Evolution, versioning and compatibility](features.md#evolution-versioning-and-compatibility-evo), [Evolution, versioning and compatibility (ww_stdlib)](std_library/stdlib_features.md#evolution-versioning-and-compatibility-evo), [Transports and protocols](features.md#transports-and-protocols-tr), [Client / host side](features.md#client-host-side-clt), [Evolution of these crates' types and the id ledger (ww_base)](std_library/features.md#evolution-of-these-crates-types-and-the-id-ledger-ww_global-evo) in the feature tracker.</p>

## Tools

A command line tool, a Python module and a GUI to work with devices and APIs without writing a client.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[`ww` command line tool](cli.md)**: List devices, show a device's API as a tree, with shell completions.
- **[Python](python.md)**: Any device from Python, its API taken from the device itself.
- **[Server scaffold](cli_reference.md#ww-api)**: `ww api scaffold` prints a stub for every handler a server needs.
- **[Device info and reports](cli_reference.md#ww-info)**: `ww info` for any device; `ww report` adds counters, logs and crash data in one file.
- **[Crash call chain](std_library/device_contract.md#crash-call-chain)**: `ww report` names a panic's or fault's call chain as `file:line` frames, inlined functions included, from the firmware's ELF, also when panics carry no location.
- **[Safe to look at](cli_reference.md#untrusted-device-and-hub-strings)**: A device or another program can't write terminal escape sequences into what `ww` prints: control characters and bidi tricks in a product string, name or label always show up as plain text.
- **[Recording a board rework](std_library/ww_board_info.md#recording-a-rework)**: `ww board rework set 7` / `clear 7` record a rework done on the bench, on any device that keeps the list; an unknown number is refused with the numbers the firmware knows.
- **[Flashing checks the board's reworks](cli_reference.md#checking-the-boards-reworks)**: `ww dfu rp flash` and `ww dfu stm32 flash` read the reworks done on the board through its ROM bootloader, compare them with the ones the new image knows, and ask before writing an image that doesn't know one. <span class="ww-status">hardware check open</span>
- **[`ww info` and allowances in the boot ROM](cli_reference.md#ww-info-in-the-boot-rom)**: On a board stuck in its boot ROM, `ww info` reads the image and the board's record straight through it and shows whether a reboot would run, run with the allowance, or refuse to boot; `ww board rework allow`/`disallow` fix it from there. <span class="ww-status">hardware check open</span>

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[API bundles](cli_reference.md#ww-api)**: `ww api save`, `check` and `diff`.
- **[Watching properties](cli_reference.md#ww-observe)**: `ww observe` and Python print updates as they arrive, with rates.
- **[Explaining bytes](cli_reference.md#ww-explain)**: `ww explain` shows every field of a buffer with its bits, from hex or a value written in Rust. <span class="ww-status">in progress</span>
- **[Link test](cli_reference.md#ww-usb-loopback)**: `ww usb-loopback` measures round trips, losses and speed both ways.
- **[`ww` through a debug probe](cli.md#through-a-debug-probe-rtt)**: `ww --rtt CHIP` runs every command against a board with no USB of its own, over RTT through its ST-LINK or other probe, with probe serials and chip names completed in the shell.
- **[Devices on one clock](cli_reference.md#ww-sync)**: `ww sync` keeps several devices on the host's UTC and shows how closely each follows and how far apart in phase they are. <span class="ww-status">hardware check open</span>
- **[Plain-unit hints](evolution/checker_tool.md#unit-suffix-hints-tool-22)**: `ww api check` and `ww api scaffold` point out a plain number whose name ends in `_ms`, `_hz`, `_bytes` and the like, and what fixed unit type it could become instead.
- **[A missing-test warning](evolution/checker_tool.md#a-missing-test-warning-tool-31)**: `ww api check` and `ww api save` warn, before anything else, when an API crate has no `tests/evolution.rs` calling `wire_weaver_evolution::Check`, so a wire break is caught by `cargo test`, not only by hand.
- **[Starting and ending a wire line](cli_reference.md#ww-api)**: `ww api line new` copies the newest line's module to the next one at `w(N+1).0` and lists it in `roots`; `ww api line retire` takes a line nobody runs out of the crate, keeping its module behind a Cargo feature (or deleting it), with its breaking crate bump.
- **[Scaffolding one root of several](cli_reference.md#ww-api)**: `ww api scaffold --root wire_v2::Thermo` picks the root by its module path in a crate with several wire lines, and the printed `ww_codegen!` call and comments name it the same way, so the output compiles.
- **[A reason for silencing a unit hint](evolution/checker_tool.md#unit-suffix-hints-tool-22)**: A `#[no_unit_hint]` can say why (`#[no_unit_hint = "reason"]`, or per argument); `ww api check --strict` and `ww api scaffold --strict` refuse a bare or empty one, naming the field and the form to use instead.
- **[ww_plugin: the plugin arguments without the hub](cli_reference.md#plugins)**: `DeviceArgs`, `Selection` and the plugin protocol helpers live in crate `ww_plugin`, which does not link `ww_hub`; `wire_weaver_cli` re-exports them.
- **[Commands register themselves](cli_reference.md#plugins)**: Each `ww` command registers itself with `inventory`; `wire_weaver_cli` (generic commands) and `ww_std_cli` (standard library commands) are libraries, the `ww` binary links both.
- **[Pins live per connector](cli_reference.md#ww-pins-tui)**: `ww pins tui` shows every pin of a board on its connector, read live, with keys to drive, pull and release them. <span class="ww-status">hardware check open</span>
- **[Pin tests from the command line](cli_reference.md#ww-pins)**: `ww pins test` finds shorts and pulls on any board serving `PinDiag`; plan files make it a pass/fail test. <span class="ww-status">hardware check open</span>
- **[Settings from a flash dump](cli_reference.md#ww-settings)**: `ww settings dump` decodes a settings store without the device.
- **[Values from text](python_advanced.md)**: Typed values written as text and turned into bytes at runtime.
- **[Dev GUI](dev_tool.md)**: Shows the parsed API and the generated code side by side; a UI for any device is prototyped. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Evolution, versioning and compatibility](features.md#evolution-versioning-and-compatibility-evo), [Client / host side](features.md#client-host-side-clt), [Client / host side (ww_stdlib)](std_library/stdlib_features.md#client-host-side-clt), [Tools (ww_stdlib)](std_library/stdlib_features.md#tools-tool) in the feature tracker.</p>

## Standard library

Ready-made traits and types for what many devices have: units, time, GPIO and buses, logs, firmware info, updates.
They are shared, so a tool that knows one of them works with every device that implements it. Units and numbers
build on [`exact_units`](https://github.com/romixlab/exact_units/blob/main/FEATURES.md), which keeps its own
feature tracker.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Device contract](std_library/device_contract.md)**: What every device answers: firmware, board, ids, user label and last reset.
- **[Recording a board rework](std_library/ww_board_info.md#recording-a-rework)**: After a rework on the bench, any host records it by number on any device serving `ww_board_info`; a firmware refuses numbers it doesn't know and reports ones it finds in its record.
- **[The rework guard](std_library/ww_board_info.md#the-rework-guard)**: A firmware never runs a board with a rework it doesn't know: before any init it blinks an error code and goes back to the ROM bootloader, unless the user allowed that rework for this very image; an image tells flashers which reworks it knows and where the board's record is.
- **[Common errors](std_library/ww_error.md)**: `CommonError` (`Busy`, `Timeout`, `InvalidArgument`, ...) shared by all APIs.
- **[Units and numbers](std_library/ww_si.md)**: `ww_si` quantities like volts, hertz and seconds, `ww_numeric` values.
- **[Units checked by the compiler](std_library/ww_si.md#unit-algebra)**: One generic quantity type with the unit in the type: `Volt * Ampere` is a `Watt`, `Volt + Ampere` doesn't build, `3300 mV` prints itself.
- **[Units known at run time](std_library/ww_si.md#units-known-only-at-run-time)**: A value whose unit is only known on the device, a configurable sensor channel, carries its unit and is shown like a typed one.
- **[Quantities as exact as `Fixed`](std_library/ww_si.md#quantity-fixed-or-anyquantity)**: A run-time `Quantity` takes binary prefixes (KiB), powers of ten and two without a name and full-scale fractions (1/65535), and carries bounds checked on decode.
- **[Angles, bytes and samples are dimensions](std_library/ww_si.md#dimensions)**: Bytes never convert into % or rad, rad/s isn't Hz, a torque isn't a joule, and samples per second aren't a frequency: the compiler and every client tell them apart.
- **[Date and time](std_library/ww_date_time.md)**: `ww_date_time`.
- **[IO family](std_library/io_family.md)**: GPIO, UART, I2C, SPI, ADC, PWM, 1-Wire and more, all in one style. <span class="ww-status">in progress</span>
- **[GPIO](std_library/ww_gpio.md)**: Pins and banks: levels, modes, pulls and edge events.
- **[CAN bus](std_library/ww_can_bus.md)**: From a simple bridge to a bus debugger.
- **[Typed CAN numbers](std_library/ww_can_bus.md#configuration)**: Bit timing in time quanta, load in ‰, buffer, FIFO and filter positions as indices: the CAN traits say what each number means.
- **[Logs](std_library/ww_log.md)**: `defmt` logs over a reliable stream, including what was logged before the host connected. <span class="ww-status">hardware check open</span>
- **[Indicators](std_library/ww_indication.md)**: LEDs off, essential or normal, and a blink to find a unit on the bench.
- **[Indicators in phase](std_library/ww_indication.md#animations-in-phase-std-50)**: Devices with a shared clock breathe and blink together, aligned to UTC, with nothing to configure. <span class="ww-status">hardware check open</span>
- **[Firmware updates](std_library/ww_dfu.md)**: `ww_dfu` for A/B, in-place and bootloader updates, tested with reboots and power cuts. <span class="ww-status">in progress</span>
- **[1-Wire](std_library/ww_onewire.md)**: Transactions and ROM search.
- **[Stock I2C drivers on the host](std_library/ww_i2c_hl.md)**: An embedded-hal I2C driver crate runs unchanged against the I2C bus of a remote device, one transaction per round trip. <span class="ww-status">hardware check open</span>
- **[More buses](std_library/ww_i2c.md)**: `ww_i2c`, `ww_uart`, `ww_memory` and the `ww_gpio_hl` helpers, usable with gaps. <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[Finite floats in clients](std_library/exact_units.md#in-ww-python-and-dynamic-clients)**: A float quantity whose type says `Finite` is refused as ±∞ or NaN in `ww`, Python and the dynamic client, and the evolution check tells when ±∞ starts to be refused.
- **[NaN-free floats in clients](std_library/exact_units.md)**: A float quantity sent or received as NaN is refused unless its type says `MaybeNan`, in `ww`, Python and the dynamic client alike; the evolution check tells when a NaN starts to be refused.
- **[Time scales in the model](std_library/units_in_the_model.md#references)**: An origin says what its zero is measured from and how that zero is known (exact, at run time, a time scale), so clients and the evolution check tell why a Unix time and a TAI or device time don't convert.
- **[Non-linear codes in clients](std_library/exact_units.md#non-linear-codes)**: A G.711 μ-law byte and other codec values read as the number they mean in `ww`, Python and the dynamic client, written by their exact code, from the codec's definition in the API model.
- **[`Offset` codes and a curve above them](std_library/exact_units.md#non-linear-codes)**: Video's narrow-range quantizers (`CodecShape::Offset`) and a transfer curve above a codec (`CurveDef`, signal to light) read into the API model, checked against the compiled codec. <span class="ww-status">in progress</span>
- **[Typed protocol numbers](std_library/ww_client_server.md)**: Observe periods, budgets, byte grants, store sizes, resource indices and resource paths of the request/event protocol are `Micros`, `Bytes`, B/s and `Index` types, the same bytes as before.
- **[Transfer curves](std_library/exact_units.md#transfer-curves)**: A video code with its transfer curve (`Transfer<Narrow10, Pq>`) in an API: clients read its signal and the light each code means, exact where it is a rational.
- **[Device time points in `Timed`](std_library/ww_si.md#timed-values)**: A timed value carries its time as a point on the device clock (`DeviceMicros<u32>` by default), four plain bytes in a column, and the host gets the full 64-bit time back across wraps.
- **[Run-time scales](std_library/ww_si.md#run-time-scales)**: A number whose exact scale is read at run time (a video stream's time base, a quantizer step) next to a unit fixed in the type: `ww_si::Scaled<U, T>` and `Ticks<T>`, refused on decode unless the scale is in its one canonical form.
- **[Typed numbers in 1-Wire and UART](std_library/ww_onewire.md)**: `ww_onewire`'s family code, byte counts, pin index and ROM search bit position, and `ww_uart`'s byte counts and idle character count, are typed instead of bare integers, so a client or `ww api check` knows what each one means and whether it is in range.
- **[Unit identity in the model](std_library/units_in_the_model.md#identity)**: A unit is known by its crate and name, the definition `unit!` compiles is the one clients read, and two units of one name in one crate are refused.
- **[Run-time units by reference](std_library/ww_si.md#units-known-only-at-run-time)**: An `AnyQuantity` names the API's units by their identity, a few bytes instead of the whole definition, and any reader with the API's model (a client, `ww`, Python, a saved tape) resolves them.
- **Levels and their rates, Np and dB**: A cable's attenuation in dB/100 m or dB/km, a resonator's damping in Np/s, is a quantity like any other, over the wire like any other; loss and gain are kept apart so a datasheet's positive number never flips sign by accident.
- **[Quantities without wide integers](std_library/ww_si.md#conversions-and-comparisons)**: On a small MCU, comparing and converting quantities with 32-bit numbers takes only 32-bit code: no 64- or 128-bit arithmetic is linked.
- **[Exact conversions without floats](std_library/ww_si.md#conversions-and-comparisons)**: A quantity converts into any prefix and integer type with the rounding and overflow you name, exactly for every scale, with no float or 128-bit code on the device.
- **[Typed numbers in `ww_stdlib`](std_library/ww_spi.md)**: Word sizes, byte and bit counts, offsets and counted things are their own types instead of a bare integer, crate by crate (`ww_spi` so far); a run-time-fixed value like an IO level moves from `Quantity` to `Fixed`. <span class="ww-status">in progress</span>
- **[ADC](std_library/ww_adc.md)**: Single reads, sample streams and triggered captures.
- **[PWM](std_library/ww_pwm.md)**: Outputs and input capture.
- **[Device time](std_library/ww_timebase.md)**: A clock with sync to a shared time source.
- **[Host clock sync](std_library/ww_timebase.md#host-sync-std-49)**: Devices without a sync line follow the host's clock: the host fits offset and rate from round trips and steers each device.
- **[I2C and SPI slave](std_library/ww_i2c.md#slave-mode)**: The device stands in for a chip on another master's bus: a register map the host fills, or answers the host sends, with every transfer reported back. <span class="ww-status">hardware check open</span>
- **[I2C master errors and limits](std_library/ww_i2c.md#transactions)**: Which byte was not acknowledged, a clock stretch timeout, a stuck bus reported and freed on request, and the adapter's limits. <span class="ww-status">hardware check open</span>
- **[Logic analyzer](std_library/ww_logic_analyzer.md)**: Triggered captures, transitions compressed. <span class="ww-status">in progress</span>
- **[Pin tests](std_library/ww_pin_diag.md)**: Shorts, pulls and floating pins on any board through one trait, the board's safety rules kept in its firmware. <span class="ww-status">in progress</span>
- **[Interned strings](std_library/ww_pin_diag.md#ww_interned)**: Names and reasons sent as a small id, the text read from the firmware's ELF on the host. <span class="ww-status">in progress</span>
- **[Signal generator](std_library/ww_signal_generator.md)**: Replays captures, drives DAC channels. <span class="ww-status">in progress</span>
- **[Counters](std_library/ww_counters.md)**: A device's `cnt` counters, named on the host from its firmware file.
- **[Recorder control](std_library/ww_recorder.md)**: Status, format, recordings, markers and settings of a logger that writes to an SD card. <span class="ww-status">in progress</span>
- **[Block device](std_library/ww_block.md)**: Read, check and write an SD card or flash by block over the link, writes under a lease. <span class="ww-status">in progress</span>
- **[Raw SD card](std_library/ww_sd.md)**: Registers, init report and raw commands with timing, for bring-up and card checks. <span class="ww-status">in progress</span>
- **[Age of a crash record](std_library/ww_firmware_info.md)**: How many boots ago the saved crash happened.
- **[Settings store](api/settings.md)**: A device's settings in its own flash, power-cut safe, readable by older and newer firmware. <span class="ww-status">hardware check open</span>
- **[Versions](std_library/ww_version.md)**: `ww_version`.

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Request and event model](std_library/ww_client_server.md)**: `ww_client_server`, the messages every call is made of.
- **[Tape blocks](https://tape.vhrd.tech/format/#tape-blocks)**: `ww_tape`: the generic blocks of track records that live feeds and log files (link captures) are made of.
- **[Tape clocks and describe](https://tape.vhrd.tech/format/#tape-blocks)**: Clocks with exact rates, clock maps, typed items and track dimensions in tapes; deterministic bytes.
- **[Tape chunk blocks](https://tape.vhrd.tech/format/#tape-blocks)**: Self-locating chunk blocks, 4 KiB aligned payloads and a CRC per 64 KiB segment, so damage costs one segment.
- **[Tape event times](https://tape.vhrd.tech/format/#tape-blocks)**: Event times stored at about 1.5 bytes per periodic item and 3.8 for bursty ones, searched and decoded in place with SIMD.
- **[Tape summary blocks](https://tape.vhrd.tech/format/#tape-blocks)**: A stored item tree for events and mergeable min, max, sum and power summaries for series, so an overview reads a few hundred KB, never the items.
- **[Tape index, close and tail](https://tape.vhrd.tech/format/#index-close-and-tail)**: Checkpoint and final indexes, a BLAKE3 content hash and a tail found from the end: a crash costs the open chunk.
- **[Tape pieces](https://tape.vhrd.tech/format/#tape-pieces)**: One recording as a chain of complete files, linked both ways; a deleted first piece reads as head cut.
- **[Positional tape reader](https://tape.vhrd.tech/format/#reading-by-position)**: Reads tapes by offset from any byte source, also in the browser over HTTP range requests.
- **[Tape sample codecs](https://tape.vhrd.tech/format/#sample-codecs)**: Series columns stored as the device sends them, or coded lossless as bit-packed deltas, read in place by any range.
- **[Tape series tracks](https://tape.vhrd.tech/format/#series-tracks)**: A series track says its sample type, scale and exact period; segments mark gaps and prefix changes, and a received `Series<Fixed>` batch becomes chunk columns without a float.
- **[Tape runs and media provisions](https://tape.vhrd.tech/format/#sessions-media-and-damage)**: Tapes on SD cards and raw NAND read past gaps, never take stale blocks for data, and survive damaged definitions.
- **[Tape payload codec](https://tape.vhrd.tech/format/#payload-codec)**: Event payloads with bit-packed lengths, raw or lz4, salvaged item by item from a damaged chunk.
- **[Tape track identity by name](https://tape.vhrd.tech/format/#track-identity)**: A track's key hashes its resource names and the keys of keyed arrays, so two runs that number things differently give the same ids.
- **[Tape summaries across scales](https://tape.vhrd.tech/format/#tape-blocks)**: Series summaries stay exact when an autoranging device changes prefix: each block states its scale, nodes merge at a common one.
- **[Tape ALP float codec](https://tape.vhrd.tech/format/#sample-codecs)**: Floats that began as decimals stored as bit-packed integers, other floats by their front bits, lossless and read in place.
- **[Tape summary kernels](https://tape.vhrd.tech/format/#tape-blocks)**: Series summaries in one vectorised pass: complex i16 at about 5 GB/s on one core, bit for bit the same results.
- **[Tape hardening](https://tape.vhrd.tech/format/#tape-blocks)**: Tape readers take damaged or hostile files without a panic, an overflow or an unbounded allocation; every limit is a named cap.
- **[Tape resume and additions](https://tape.vhrd.tech/crashes/#resuming-the-file)**: A crashed tape piece resumes to the bytes of an uninterrupted write; tracks added mid-piece reach every later describe copy.
- **[Tape pieces on 4 KiB](https://tape.vhrd.tech/#pieces-and-chains)**: Closed tape pieces end on a 4 KiB boundary, so many pieces clone back to back into one file without copying a byte.
- **[Tape definitions near their use](https://tape.vhrd.tech/#state-tracks-and-near-use)**: Tracks, clocks, bundles and state items are records on track 0 and state tracks, put again only where not close behind their use.

<p class="ww-features__more" markdown>Details and what is planned: [Standard APIs (ww_stdlib)](std_library/stdlib_features.md#standard-apis-std), [Evolution, versioning and compatibility](features.md#evolution-versioning-and-compatibility-evo), [Base types and quantities (ww_base)](std_library/features.md#base-types-and-quantities-ww_si-ww_numeric-ww_version-ww_date_time-ww_error-std), [Format (ww_tape)](tape/features.md#format-fmt), [Tracks, clocks and definitions (ww_tape)](tape/features.md#tracks-clocks-and-definitions-track), [Codecs (ww_tape)](tape/features.md#codecs-codec), [Summaries (ww_tape)](tape/features.md#summaries-sum), [Writer (ww_tape)](tape/features.md#writer-write), [Reader (ww_tape)](tape/features.md#reader-read), [Files and locks (ww_tape)](tape/features.md#files-and-locks-fs) in the feature tracker.</p>

## Examples

Working projects to start from: host programs, firmware for a few boards, and the crate layout to copy.

<p class="ww-features__level"><span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span></p>

- **[Project layout](api/folder_structure.md)**: An `<name>_api` crate with the trait and types, next to the code that serves and calls it.
- **[Host examples](examples/examples.md)**: Blinky, all GPIOs, UART, and blinky from Python.
- **[Firmware examples](examples/examples.md)**: STM32 Nucleo boards, RP2040 / RP2350 and QEMU, over USB, RTT, WebSocket and UDP.
- **[Serialization only](examples/examples.md)**: `shrink_wrap` without an API, and a comparison with other formats.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **[UDP over a USB network adapter](examples/examples.md)**: An RP2 board and its host side.
- **[Motor controller demo](examples/motor_demo.md)**: 52 observed properties at three rates on a simulated motor, in the motor_sim repo.
- **[Counters in firmware](std_library/ww_counters.md)**: `cnt` counters in every probe-connected board example.
- **[Settings demo](api/settings.md)**: Settings that survive a reboot, on the host and on two Nucleo boards.

<p class="ww-features__more" markdown>Details and what is planned: [Example and cases (ww_tape)](tape/features.md#example-and-cases-ex), [Examples](features.md#examples-ex) in the feature tracker.</p>

## Testing

How WireWeaver is kept working: every change runs through CI, against the real client and server code.

<p class="ww-features__level"><span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span></p>

- **Continuous integration**: Formatting, lints, docs, tests, semver checks and firmware builds on every change.
- **This page needs no edit for another repository**: Landing a feature in shrink_wrap, ww_base, ww_tape or ww_stdlib never breaks this repository's gate: their own trackers are taken as they give them.
- **Integration tests**: Every API feature tested through the real host event loop and device server.
- **Fuzzing**: The framer and `shrink_wrap`'s readers fed random input; more targets to come. <span class="ww-status">in progress</span>
- **Test coverage**: A few remaining cases. <span class="ww-status">in progress</span>

<p class="ww-features__level"><span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span></p>

- **[Conformance matrix](serdes/conformance.md)**: One corpus of golden bytes every wire-format implementation must match: derive (owned and borrowed), dynamic client, Python, C; guarded against a sealed case changing and against a wire kind going untested.
- **[Lossy test medium](transport/udp.md#simulated-udp-in-tests)**: Simulated UDP that drops and reorders the host's datagrams on a plan the test gives, on the paused clock. <span class="ww-status">in progress</span>
- **[Dispatcher golden replies](serdes/conformance.md)**: One corpus of golden request/reply bytes covering every resource kind the dispatcher serves, checked against a sync and an async server and guarded against a sealed case changing.
- **[Code size bench](serdes/code_size.md)**: Firmware images per case for RISC-V and Cortex-M, measured on every change.
- **Binary size tracking**: Flash and RAM per feature in CI. <span class="ww-status">in progress</span>
- **[Units on a microcontroller](articles/units_budget.md)**: What typed units (`Fixed`) cost in instructions, flash and stack against plain integer fields, from a harness anyone can re-run.
- **[Flash of a server](articles/flash_budget.md)**: What a server costs in flash on a Cortex-M0+: the floor and the bytes of each more resource by kind, from a harness anyone can re-run.
- **[Power cuts on STM32](api/ecc_flash.md)**: A flash store reset thousands of times mid-write on real hardware. <span class="ww-status">in progress</span>

<p class="ww-features__more" markdown>Details and what is planned: [Testing, CI and performance](features.md#testing-ci-and-performance-test), [Wire format (shrink_wrap)](serdes/features.md#wire-format-shrink_wrap-sw), [Testing and code size (shrink_wrap)](serdes/features.md#testing-and-code-size-test) in the feature tracker.</p>
