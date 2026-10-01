# WireWeaver features and roadmap

This file is the single source of truth for what WireWeaver supports, what is being worked on and what is
planned, for humans and AI agents alike. It replaces the external task tracker.

## How to use this file

- **Status** of each item:
    - ✅ done
    - 🚧 in progress or partially done (the note says what is missing)
    - 📋 planned
    - 💡 idea, not committed to
    - ⛔ blocked (the note says on what)
    - 🔍 probably done or obsolete, needs a check before closing
- **Target** is the release an item is planned for: `v0.5` is the next release (the workspace is at 0.5.0,
  crates.io has 0.4.0). `v0.5.x` is the next compatible release after it: `v0.5.1` if nothing turns out to be
  breaking, otherwise the next minor. No tag means not scheduled. `grep '`v0.5`' FEATURES.md` gives the release
  scope.
- Items are grouped by area. Each area lists what works first (✅), then open items ordered by target.
- When finishing work, update the item in the same change: mark it ✅, move it up to the done items of its
  area and add a pointer (docs page, crate, test or example). Don't delete done items. Move items that turn
  out to be obsolete to [Dropped and superseded](#dropped-and-superseded) with a one-line reason.
- New ideas go into the matching area as 💡. Small code-level gaps stay as `TODO` comments in the source;
  only those that limit users or block a feature get an item here.

## Wire format (`shrink_wrap`)

- ✅ **Structs, plain enums and data enums**, discriminant type selection with `ww_repr`.
- ✅ **`Option` and `Result`**, also inside `Vec`, as one-bit flags.
- ✅ **`String` and `Vec<T>`**, with special handling of `Vec<u8>`.
- ✅ **Fixed size arrays, tuples and the unit type `()`**.
- ✅ **Bit-aligned integers**: nibbles and `u1`..`u32` (`un`); `un8`/`un16` are forwarded to `un32` for smaller
  flash.
- ✅ **Variable length numbers**: `UNib32`, `UVlq32`, and `UVlq32Backfill` for values known only after the rest
  is serialized.
- ✅ **Borrowed and owned types**: `Sized` and `Unsized` traits, `RefStr`/`RefVec`, borrowed types by default and
  `Owned` variants generated automatically.
- ✅ **Evolvable by default**: every struct and enum is `Unsized`; `final_structure`, `self_describing` and
  `sized` are opt-ins.
- ✅ **`TailBytes` / `TailBytesOwned`**: bytes that take up the rest of the buffer, written without a size; only
  valid as the last item. Generated API code puts them in place of byte slices, e.g. for return values and
  property values.
- ✅ **`EitherAnyVec`**: zero-copy, no-alloc array of `Either` elements of any types, with the L/R flags packed
  in groups of 8 instead of padding each element. Packs e.g. a list of `Result`s tightly; used for multi-read
  and multi-call results (only multi-read exists so far).
- ✅ **Alloc writer** `BufWriterOwned`, `SerializeShrinkWrap` for `&[u8]`, `String` as UFS.
- ✅ **In-place builders** used by the server codegen (event and error builders), and the manual
  patch-the-discriminant-later pattern (`docs/serdes/showcase.md`).
- ✅ **Binary format documented** in `docs/serdes/shrink_wrap.md`, worked tricks in `docs/serdes/showcase.md`.
- ✅ **Usable standalone** as a dense data storage format (`examples/minimal_shrink_wrap`,
  `examples/compare_wire_formats`).
- 🚧 **Handle end of buffer when reading evolved fields** · `v0.5` — fields with `#[default = ..]` already fall
  back to the default, but on _any_ read error (`Field::handle_eob` in
  `shrink_wrap_derive/src/codegen/item_struct.rs`), not only on end of buffer. Missing: fall back only on
  end-of-buffer errors, and only for fields added in a later version; `Vec`/`String`/`Option` fields appended
  without a default should read as empty/`None` from an older buffer (`TODO` in
  `shrink_wrap_derive/src/codegen/ty.rs`).
- 📋 **`FutureVersion` enum variant** · `v0.5` — a catch-all variant holding the unknown discriminant (and the
  payload bytes for `Unsized` enums), so an older reader can pass on or report an enum value from a newer
  version instead of failing with `EnumFutureVersionOrMalformedData`. Notes:
    - the enum must already be `Unsized` to add new variants with data later; unit variants then write a 0
      byte size each;
    - a `sized` enum can only get new unit variants;
    - add to all enums by default, unless all discriminants of the repr are taken or an attribute opts out.
- 📋 **Const / magic type** · `v0.5` — a simple type with a generic parameter that serializes a constant value or
  byte string and checks it on deserialization, failing on a mismatch. For example the `magic` field in
  `ww_self` (a plain `u32` today).
- 📋 **Size-of-enclosing-type field** — a special type that reserves space and is backfilled with the size of
  the type it is in once that is serialized, building on `UVlq32Backfill`. Lets a reader skip or bound the
  enclosing value without parsing it.
- 📋 **Bounded sizes: `#[max_size]`, bounded `String`/`Vec`** · `v0.5.x` — from `docs/types.md` and
  `docs/api/arrays.md`. Enables worst-case buffer size analysis (see [Codegen](#api-model-and-codegen)).
- 📋 **Sub-type numbers** · `v0.5.x` — numbers restricted to a range, like `10..=25`, or a set with gaps, like
  `0..=8, 12, 16`; checked on serialization and deserialization, possibly using fewer bits on the wire
  (`docs/types.md`, "Subtypes").
- 📋 **Multi-dimensional arrays** (2D, 3D).
- 📋 **Fuzz `shrink_wrap`** — `fuzz/` only covers `ww_framer` today. Round-trip generated types and feed random
  bytes into readers.
- 🔍 **Tuple enum variants evolution** — check that tuple variants evolve as `docs/evolution/rules.md` says
  (fields can be appended) and add a test.
- 💡 **ZigZag encoding for signed variable length numbers** — consider ZigZag (as in protobuf) so that small
  negative values stay short, e.g. signed counterparts of `UNib32`/`UVlq32`.
- 💡 **In-place builder for server handlers** · `v0.5.x` — a new step-by-step / partial builder (e.g. for a
  `RefVec`) might be useful with the new `Context` in server handlers, to write replies and events straight
  into the buffer. It might not work out.
- 💡 **Map support** — `HashMap`/`BTreeMap`, probably as a `Vec` of `(K, V)`.
- 💡 **In-place editing** of serialized data.
- 💡 **Compression wrappers** · `v0.5.x` — in-place dictionary compression as `Compression<T>`; delta,
  delta-of-delta and float XOR encodings for time series (strings?).
- 💡 **Flatten support** — apply a type's layout across struct/enum boundaries.
- 💡 **`derive_shrink_wrap` on a whole module at once**. For definitions in separate files with own syntax, see
  [standalone definition files](#api-model-and-codegen).

## `derive_shrink_wrap` macro

- ✅ **`#[derive_shrink_wrap(..)]` attribute** with clear syntax, no hidden magic and good error messages
  (`docs/serdes/derive.md`); replaced `#[derive(ShrinkWrap)]`.
- ✅ **Discriminant checks**: discriminants must fit `ww_repr`, implicit ones are numbered like in Rust.
- ✅ **`#[default = ..]` for evolved fields** and `#[flag]` relocation.
- 📋 **`#[since = "x.y.z"]` on fields** — generate correct evolution code from it (`TODO` in
  `shrink_wrap_derive/src/lib.rs`).
- 📋 **Check that `TailBytes` is last** · `v0.5` — like `UVlq32Backfill` is checked to be the first field
  (`check_backfill_position` in `shrink_wrap_derive/src/transform/util.rs`). Today only its doc comment warns,
  and a misplaced `TailBytes` silently swallows the fields after it. The derive can reject it anywhere but as
  the last field of a struct or enum variant, and nested in `Option`, `Vec`, tuples or arrays. Not visible to
  it: a struct ending in `TailBytes` used as a non-last field of an outer `sized` / `final_structure` type
  ("last in the first `Unsized` type on the way up"); that needs a marker on the type, e.g. an associated
  const, and a compile-time assertion in the outer type.
- 📋 **Prefix enum discriminants** · `v0.5.x`.
- 💡 **`#[cfg(feature = "..")]` on enum variants** — hard to handle with dynamic serdes in general, but might not
  be that bad for specific use cases.

## API model and codegen

- ✅ **Methods, properties, streams and sinks**, nested traits (`docs/api/overview.md`).
- ✅ **Arrays of resources** with `ValidIndices` and a built-in array size getter returning a range or list.
- ✅ **Global traits** from `ww_stdlib` (`impl Trait`) and trait attachments (`examples/all_gpio`).
- ✅ **Results**: methods return `RpcResult` (`Ready`, `Defer`, `Unimplemented`), properties return
  `GetResult`/`SetResult`, with user errors and property errors.
- ✅ **Request `Context` in every handler**: seq, medium, event out, deferred replies.
- ✅ **Event, `EventKind` and error builders**.
- ✅ **`match` on index slices**, the base of multi-read support.
- ✅ **`build.rs` codegen** via `gen_server`/`gen_client`, as an alternative to the proc macro.
- ✅ **Full name chains** in generated names (fixed the `event_sideband` collision).
- ✅ **Type paths from Cargo**: trait source path, external types in API crates as full paths.
- ✅ **Sync and async servers, async / blocking / promise clients** from the same trait.
- 📋 **Calculate max path depth in server codegen** · `v0.5` — `MAX_DEPTH` is hardcoded to 16 in
  `wire_weaver_core/src/codegen/api_server.rs`.
- 📋 **Error system, remove `unwrap`s from codegen** · `v0.5` — report errors with spans instead of panicking.
- 📋 **Load API crates from crates.io** · `v0.5` — the crate walker handles path and workspace dependencies;
  `crate = "version"` dependencies hit a `todo!()` in `wire_weaver_core/src/transform/crate_walker.rs`. Also
  dependencies from git (backlog).
- 📋 **Return streams from functions** · `v0.5.x` — related idea: model streams as functions that can be called
  to change parameters, subscribe, stop.
- 📋 **Feature groups** · `v0.5.x` — resources behind features (all enabled by default), connected to the user
  crate's features in `ww_codegen!`. Related: `#[cfg]` in enums, see [derive macro](#derive_shrink_wrap-macro).
- 📋 **Methods arguments to a user struct** · `v0.5.x`.
- 📋 **Size analysis** · `v0.5.x` — maximum buffer usage in the worst case, auto-calculated buffer sizes for events,
  arguments and outputs. Depends on bounded sizes.
- 📋 **Change dispatcher match order** to process common requests faster · `v0.5.x`.
- 📋 **Trait paths on the server side** — `TODO` in `api_server.rs`.
- 📋 **`no_alloc = false` servers** — don't compile today.
- ⛔ **Implement the same trait several times → enum selector** · `v0.5.x` — stalled. At least print a proper error
  when a trait is implemented twice (`tests/traits_api`).
- ⛔ **Index chain array → named arguments** · `v0.5.x` — stalled. Generating ad-hoc structs with named index
  fields might be a good ergonomic: users see the names through IDE lookup instead of guessing array indices.
- 💡 **Guarded calls for critical resources** · `v0.5.x` — for safety-critical actions, such as starting a motor
  or switching high power, a request that happens to match predefined bytes must not be enough. Add an extra
  check, e.g. a CRC over the resource identity and arguments (a "strict mode" per resource), or a
  request/response challenge before the call is executed.
- 💡 **Refactor codegen to use an IR** · `v0.5.x`.
- 💡 **Generate C** · `v0.5.x`, and an **FFI example**.
- 💡 **Standalone definition files with custom syntax** — APIs and types in their own files, with a friendlier
  syntax: nested levels, no macro calls, SI units. Tried twice already, approach with care:
    - at the very beginning of this repo, a YAML based language;
    - later, a big effort to support a custom syntax, which turned into a project of its own and took a lot of
      resources away from the actual work.

  Plain Rust with `#[ww_trait]` and `#[derive_shrink_wrap]` is the current answer; revisit only with a concrete
  benefit that outweighs the cost of a parser, tooling and IDE support.

## Server / device side

- ✅ **`no_std`, no-alloc server**, IO-free generated code.
- ✅ **Sans-IO `DeviceLink`**, async `Server` with `wait()`/`handle()` for a user-owned event loop, and
  `blocking::Server`.
- ✅ **Fully split TX and RX flows** for every transport (fixed the half-duplex interlock), framing without IO.
- ✅ **User handlers called straight from the RX loop**, transport glue extracted into `ww_device`.
- ✅ **Device descriptors**: serial, WireWeaver version, API id, version, hash and user label in the USB interface
  string.
- ✅ **Media**: USB (embassy), RTT, WebSocket (also on `embassy-net`).
- ⛔ **One crate per role for firmware and host** · `v0.5` — blocked on generated code using absolute paths.
  Agreed direction, instead of putting everything into `wire_weaver` behind features or splitting by
  std/no_std:
    - `wire_weaver` stays lean (shrink_wrap, macros, `ww_version`, result types) since every API crate and
      `ww_stdlib` crate depends on it;
    - `ww_device` re-exports `wire_weaver`, `ww_framer`, `ww_link` and gets features `usb-embassy`, `rtt`, `udp`,
      `ws`, `defmt`, `embassy-time`, `std`;
    - `wire_weaver_client` re-exports `wire_weaver` with transport features.

  Requires: codegen emitting paths through the role crate (or a `crate = ".."` argument / `proc-macro-crate`;
  `WIRE_WEAVER_REEXPORTS` in the crate walker is partly there), and `wire_weaver_usb_embassy` building in the
  root workspace or merging into `ww_device`. Separately consider making `wire_weaver`'s `std` default off.
- 📋 **Handle USB suspend/resume** · `v0.5` — run until suspend, wait for resume, loop
  (`mcu/wire_weaver_usb_embassy`). Later (`v0.5.x`): detect the host going to sleep and keep the connection,
  informing the device.
- 📋 **UDP device medium** · `v0.5` — the host has a `udp` transport again, `ww_device` only has RTT and
  WebSocket. Needed for the NCM UDP examples.
- 📋 **Collect error codes into an ELF section** for easier error decoding · `v0.5.x`.
- 📋 **Link statistics** in `ww_device` (`TODO` in `link.rs`).
- 💡 **std server on Tower** · `v0.5`.
- 💡 **IPC over USB / multi-app devices / USB multiplexer** · `v0.5.x`.
- 💡 **FPGA server** · `v0.5.x`.

## Client / host side

- ✅ **`std` client** with async, blocking and promise flavors, prepared call objects, no `root()` needed.
- ✅ **Generic sans-IO event loop** over a `Transport`, independent TX and RX flows.
- ✅ **USB transport** on nusb: endpoint addresses from descriptors, bulk with lower priority, vec recycling.
- ✅ **RTT transport** over a debug probe, control block address from an ELF file.
- ✅ **WebSocket, UDP and in-process transports**.
- ✅ **`DynClient`** and dynamic resources, for APIs known only at runtime.
- ✅ **Introspection cache** keyed by API hash with and without docs, with a download timeout.
- ✅ **Compatibility check on connect**: each resource is checked against device introspection when the API hash
  differs.
- ✅ **Connection selectors**, clear errors when no device is found, `disconnect()` releases the transport.
- 🚧 **Trait calls that work on any device** · `v0.5` — serial, user label, a CLI that works with all devices.
  `ww list` with filters and USB interface string metadata are done; calling `ww_stdlib` traits generically
  from the CLI is not.
- 📋 **Typed property handles** · `v0.5` — `RwProperty`/`RoProperty`/`WoProperty` so that
  `.prop_name().read()`, `.prepare_write(1).blocking()`, `.asynch().await` work.
- 📋 **Stream receive timeout** · `v0.5` — and a beat timeout between updates.
- 🔍 **Typed attachments with `TraitMarker`** · `v0.5` — trait attachments exist (`Attachment`,
  `examples/all_gpio`); check whether a typed marker is still wanted.
- 💡 **`no_std` client** (`client = "raw"`) — documented as not working in `ClientModel::Raw`, its test is
  commented out in `tests/traits`. Its generated code serializes arguments into a fixed 128 byte scratch and
  then calls `.to_vec()`, which isn't `no_std`; it should write into a caller-provided buffer instead. The std
  clients (`std_client`, `trait_client`) already serialize with `to_ww_bytes_owned()` and have no size limit.
- 🚧 **Multi read/write/call** · `v0.5.x` — `MultiRead` exists as a prototype; multi write and call are missing,
  and full multi-property support.
- 📋 **Verify the hash of a cached API bundle** · `v0.5.x` (`wire_weaver_client/src/local_registry.rs`).
- 📋 **Reliable requests over unreliable media** · `v0.5.x` — a second, non-increasing seq number to guard against
  a lost reply for a request that was executed, when executing twice is not OK. Relevant for UDP.
- 🔍 **Backpressure** — commands already go through a bounded channel (`ClientConfig::cmd_queue_size`), so
  user code should block or yield when the queue is full; add tests for it. Running out of request ids
  currently fails the request with an error instead of waiting (`TODO` in `event_loop/core/tx.rs`).
- 📋 **Connection priority** in the client config (`TODO` in `config.rs`).

## Transports and protocols

- ✅ **`ww_framer`**: packs many small messages into one packet or splits a big one across packets; U2Head with
  1 byte overhead, CRC, recovery from missed frames, fuzzed.
- ✅ **`ww_link`**: link setup, device info, API version exchange.
- ✅ **USB**, **RTT**, **WebSocket**, **UDP** (host side) and **in-process**.
- ✅ **USB CDC-NCM**: WebSocket over a USB network adapter, with a DHCP server so the host gets an address by
  itself, ping and a hello page over HTTP (`examples_mcu/rp2`, `ww_ws_ncm`).
- 💡 **USB CDC-NCM extras** — test on Windows, macOS, iOS and Android; serve files from an SD card over HTTP;
  mass storage with a README.
- 📋 **CAN** · `v0.5.x` — server and client, using CANopen (`docs/transport/overview.md`).
- 📋 **UART** · `v0.5.x`.
- 💡 **IP / WireGuard** · `v0.5.x`.
- 💡 **EtherCAT**.

## Introspection (`ww_self`)

- ✅ **API model AST** with syn-style `Visit`/`VisitMut`, sent by devices as introspection data.
- ✅ **Bundles** recording crate versions, signatures of skipped traits and types.
- ✅ **Relocated `#[flag]` fields** in introspection data and dynamic serialization.
- 📋 **Bit-aligned integers in `ww_self`** · `v0.5` — `un`/`in` types (`u1`..`u32`, nibbles) are not supported in
  the introspection model yet, so the dynamic client and Python can't handle them. Add them, with tests
  through dynamic serdes.
- 📋 **Documentation pass on `ww_self`** · `v0.5` — doc comments are missing (`TODO` in `ww_self/src/lib.rs`).
- 📋 **`sized` / `final_structure` on `ww_self` types** · `v0.5` — where a type is unlikely to evolve, to make
  introspection data smaller. Can't be undone later, so decide per type.
- 💡 **Compress `ww_self` strings with a pre-shared dictionary** — partly addressed by leaving snapshot-known
  traits and types out of introspection data.

## Evolution, versioning and compatibility

- ✅ **Evolution rules** in `docs/evolution/rules.md`: crate layers and version bumping.
- ✅ **SHA signatures** of APIs and types, hashing only the major version (or `0.minor`), version independent.
- ✅ **Crate version recorded in bundles**.
- ✅ **Global traits referenced by hash**, global type registry (`ww_global`).
- ✅ **API snapshots** of `ww_global` and `ww_stdlib` (`wire_weaver_snapshots`, `ww api save`), left out of
  introspection data and put back by the client.
- ✅ **Evolution checker and report**: `ww api check` / `ww api diff` (`docs/evolution/checker_tool.md`).
- 💡 **Git dirty flag** — not implemented, although the old tracker had it as done: neither `ww_version` nor
  codegen record git state. Uncommitted API changes are still caught, since they change the API's SHA
  signature. Firmware git info (sha, dirty, branch, tag) can be exposed through `ww_firmware_info`'s
  `bedrock_build_info` stream. Decide whether the link or version also needs it.
- 📋 **Snapshots of crates.io crates** — depends on "Load API crates from crates.io" above.

## Tooling: CLI, GUI, Python

- ✅ **`ww` CLI**: `list` with filters, `introspect` as a resource tree (`--raw-as-sent`, `--types`), dynamic shell
  completions (`docs/cli.md`).
- ✅ **`ww api save` / `check` / `diff`** for API bundles and snapshots.
- ✅ **`ww api scaffold`** generates server handler stubs and the matching `ww_codegen!` call.
- ✅ **Python module** talking to any device, API built from introspection, dynamic values serdes, RTT built in
  (`wire_weaver_py`, `docs/python.md`).
- ✅ **Runtime literals**: values to bytes at runtime (`eval`, dynamic values).
- 🚧 **Dev GUI / web tool** · `v0.5.x` — `wire_weaver_tool` (egui, Trunk) shows the AST and generated code. Goals:
  a web tool like the pest playground, a dynamic UI for any API (MVP prototyped), widgets per type, mockup
  server UI. See `docs/dev_tool.md`.
- 💡 **Generated Python wheel for a specific API** — nothing is generated for Python yet:
  `examples/blinky_py` is a hand-written pyo3 wrapper around the Rust client, and `wire_weaver_py` is dynamic.
  Generating typed pyo3 bindings from an API crate might be useful (see the `TODO` in
  `docs/api/folder_structure.md`).
- 📋 **`ww` inspect / read / write all GPIOs on any device** · `v0.5.x`.
- 📋 **Buffer visualizer / "explain" encoding** · `v0.5.x` — record operations on buffers so that each bit can be
  traced to an item, show it using the AST and dynamic values (`TODO` in `wire_weaver_core/src/eval.rs`).

## Standard library (`ww_stdlib`)

- ✅ **`ww_client_server`** — the request/event model, pruned of sideband/subscribe/rate, variable length seq
  numbers.
- ✅ **`ww_version`**.
- ✅ **`ww_si`** and **`ww_numeric`**.
- ✅ **`ww_date_time`**.
- ✅ **`ww_gpio`**.
- 🚧 **`ww_gpio_hl`** — usable, some APIs are still missing.
- 🔍 **Interrupt left enabled in `ww_gpio_hl`** · `v0.5` — the falling edge helper documents that it leaves
  interrupts enabled; decide whether that is the intended fix or a bug.
- 🚧 **Prototypes**, to be completed later: `ww_can_bus`, `ww_i2c`, `ww_uart`, `ww_counters`, `ww_indication`,
  `ww_ll`, `ww_log_bare_metal`. Known gaps are `TODO`s in their sources (e.g. SMBus and 7-bit addresses in
  `ww_i2c`, capabilities and properties in `ww_can_bus`).
- 💡 **Stubs**, for later: `ww_spi`, `ww_dfu`, `ww_board_info`, `ww_firmware_info`, `ww_user_info`, `ww_uid`.
- 💡 **Device state machine trait** like CANopen.
- 💡 **Register description registry** — WireWeaver for describing device registers.

## Examples

- ✅ **Host examples**: `blinky` (+ evolved API), `all_gpio`, `uart`, `blinky_py` (hand-written pyo3 wrapper).
- ✅ **Serialization only**: `minimal_shrink_wrap`, `compare_wire_formats`.
- ✅ **MCU examples**: nucleo_g0b1re, nucleo_h743zi2, usb_stm32h725ig, mcu_qemu, rp2 (USB, RTT, WebSocket over
  NCM).
- ✅ **Project layout** of paired `<name>_api` / `<name>` crates (`docs/api/folder_structure.md`).
- 📋 **Example without async** · `v0.5` — `ww_device::blocking::Server` exists, no example uses it.
- 📋 **Example with two media** · `v0.5`.
- 📋 **NCM UDP example** and **NCM WebSocket + UDP example** · `v0.5` — need the UDP device medium.
- 💡 **WebSocket over Ethernet example** for some board · `v0.5.x` — the `ws` medium already runs on
  `embassy-net`, so Ethernet needs no transport work, only an example.
- 📋 **Reconnect demo** · `v0.5.x` — keep reconnecting with streams and UI feedback; `disconnect_keep_streams`.
- 📋 **Multi-property observe demo** · `v0.5.x` — timers and previous values, analog and digital signals, a
  dynamic rate for each property.
- 📋 **Hubris based example** with sync code · `v0.5.x`.
- 📋 **Advanced example / project template**.

## Documentation

- ✅ **Docs site** (zensical): wire format, derive macro, showcase, API, evolution rules, transports, CLI,
  Python, std library pages.
- 📋 **Run code in docs as tests** · `v0.5` — set up example tests from the `.md` files.
- 📋 **Link every feature to a docs page with an example** · `v0.5` — this file can serve as the index.
- 📋 **README pictures and showcases** · `v0.5` — including a picture showing density compared to MessagePack and
  friends; fill the `TODO`s in `README.md`.
- 📋 **Open docs `TODO`s** — export of all API levels with ids (`api/addressing.md`), splitting traits into
  multiple files (`api/traits.md`), interrupt vs bulk (`transport/usb.md`), `T` → struct containing `T`
  (`evolution/rules.md`).
- 🔍 **Docs for `ww_stdlib` crates and resource doc comments** · `v0.5.x` — the crates are now publishable, so
  docs.rs builds them; check whether generated resource types carry doc comments.

## Testing, CI and performance

- ✅ **CI on GitHub Actions**: fmt, sort, typos, clippy, rustdoc, nextest, doctests, semver-checks, MCU builds.
- ✅ **Integration tests** against the real host event loop over `in_process`, one crate per API feature.
- ✅ **Device side end-to-end tests** with real framers on both ends.
- ✅ **Timeout tests** for methods, properties and streams (async, blocking, promise).
- ✅ **Method tests**: calls, unimplemented, deferred replies, deferred not answered.
- ✅ **Property tests**: multi-req and single-req servers, user errors.
- ✅ **Stream tests**: byte buffers, basic and user types, open/close, sideband both ways, sinks, arrays of
  streams.
- ✅ **Framer fuzzing** (`fuzz/`).
- 🚧 **Remaining test coverage** · `v0.5`:
    - [ ] properties: unimplemented, observe stream
    - [ ] streams: promise timeout
    - [ ] methods: return type evolving to a struct, adding arguments
- 🚧 **Binary size tracking** · `v0.5` — flash and RAM per feature for a set of examples, in CI. Consider
  compile-time construction so that only final bytes remain.
- 📋 **Keep an eye on coverage** — around 70% on average as of 2026-10-01, the important crates are at
  80-90%, `ww_framer` at 97%. Don't let it drop: add tests with new features and fixes, and raise the crates
  that are behind.
- 📋 **Coverage with hardware in the loop** · `v0.5.x`.
- 📋 **TODOs round** · `v0.5.x` — go through the `TODO` comments in the source.
- 📋 **Update all dependencies** · `v0.5.x`.
- 📋 **Measure serialization speed** on MCU and host (LE vs BE, array access vs pointers, `UNib32`
  implementations).
- 💡 **Host queue on a slow VM** — does it lead to fewer or no missed packets?

## Dropped and superseded

- **Document things** (old notes) — superseded by `docs/serdes/shrink_wrap.md` and `docs/evolution/rules.md`;
  the notes described an earlier format (field ids, `vlu16n_rev`). Remaining ideas moved to their areas.
- **LEB numbers** — superseded by `UNib32` and `UVlq32` (`ULeb128` is still in the derive AST, unimplemented).
- **Field ids** — decided against for now: fields are identified by position, which works well, and ids would
  add confusion together with flag relocation. The `id` leftovers in `shrink_wrap_derive/src/transform/` can
  go.
- **Option and Result taking two id numbers** — dropped with field ids; flags are separate fields.
- **Put length in front into a specified field** — done as `UVlq32Backfill`.
- **CLI common blocks AST generation** and **excluding common blocks from the AST** — done as API snapshots.
- **Compatibility check using AST signatures on connect** — done.
- **Introspect as a trait, removed from `ww_client_server`** — won't do.
- **WebUSB descriptors** — only some browsers support WebUSB and it needs separate host support; NCM is more
  appealing, especially with pre-programmed unique IPs.
