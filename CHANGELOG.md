# Changelog

The history of WireWeaver as a whole, from its first YAML sketches to today, newest first. For the exact changes of
a released or upcoming crate version, read that crate's own `CHANGELOG.md` (`wire_weaver/`, `wire_weaver_core/`,
`shrink_wrap/`, `ww_stdlib/*`, ...): they are the authoritative record from 0.4.0 on. This file summarizes, per
version or era, what was added and changed, and which ideas first appeared there and are still part of WireWeaver.

The project changed names several times: AML (2017-2019, before git), vhapi (2020), vhL with xPI and vhl-stdlib
(2021-2023), and wire_weaver with shrink_wrap since May 2024. The version strings below are the workspace versions
from `Cargo.toml` at the time; only 0.4.0 was published to crates.io.

## [Unreleased]

### Added

- `ww explain WHAT HEX` (TOOL-9): which bits of a buffer hold which value, as a tree of every field with its bit
  range (front bits and back nibbles of lengths and sizes), raw bits, type and decoded value, padding included, and
  a hex map coloured by field. WHAT is a type, a method's arguments (`--return`: its return value), a property's
  value or a stream's item; the API comes from the device, `--crate` or `--bundle`. The library side is
  `ww_self::explain` (`explain`, `explain_fields`: `Part`s with their ranges, for a GUI later), on top of
  `shrink_wrap::BufReader::cursors_in`.
- Values and calls as Rust expressions (TOOL-9): `ww explain Config --value 'Config { mode: Mode::Fast, gains: [1.0,
  2.5] }'`, `ww explain 'motor.set_speed(1.5, Dir::Cw)'`, `ww explain 'motor.limit = 3'` encode first, then explain.
  Literals take the field's type (`5` is a `u16`, a `ub3` or an `f32`), errors name the field (`argument a: 200 is
  out of range for ub7`). Library: `ww_self::expr` (feature `expr`): `parse_value`, `value_from_expr`, `parse_call`.

- Wire-format conformance matrix (TEST-23): one corpus of cases (`tests/conformance/corpus.json`: type, value,
  golden bytes) run by `just test` and CI through every wire-format implementation: the Rust derive, the `ww_self`
  dynamic client, Python and C (the runtime, the generated `fixed_layout` and `head_tail` readers). Gaps are marked
  per case and listed in `docs/serdes/conformance_matrix.md`; it replaces the planned codegen IR (API-25 dropped).
  It found `fixed_layout` types rejected in APIs (fixed in `wire_weaver_core`) and unknown enum variants failing in
  Python (fixed in `wire_weaver_py`). See `docs/serdes/conformance.md`.
- Two guards on the conformance corpus: `frozen_bytes` (TEST-24) fails `just test` when a case's `bytes`,
  `direction` or `bytes_from` changed, or the case disappeared, since the baseline commit (`CONFORMANCE_BASE` when
  CI sets it to the pushed range's prior commit on a push to master, else the merge base with main) — a hand edit
  or a reset to `"?"` used to re-seal silently. The baseline is parsed with a minimal, forward-compatible schema
  so a field added or renamed since doesn't turn the guard off; a baseline that isn't a corpus at all fails loudly
  instead. The escape for a deliberate wire change is a new or reworded `"wire_change": "EVO-n / reason"` on the
  case (reusing the same mark for a later, unrelated change doesn't count), shown in the matrix report. `coverage`
  (TEST-25) fails when a wire-relevant `ww_self::Type` variant or `shrink_wrap_derive` directive is exercised by
  no case and not listed as a gap; found two variants no prepared bundle ever carries (INT-11) and two directives
  with no case yet (SW-67). Both depend only on the repo's own git, print why and pass when there is nothing to
  compare (no git, a shallow clone, the corpus being new). See `docs/serdes/conformance.md`.
- shrink_wrap / ww_self: a dynamic `Series` writer (CLT-29): `ValueOwned::ser_shrink_wrap_dyn` / `ser_series_dyn`
  write a `Series` from its items with the bytes the typed writer gives for the same `CodecPolicy`, so a dynamic
  client and Python can send `Series` arguments and properties. See the `shrink_wrap` and `ww_self` changelogs.
- wire_weaver_client / wire_weaver_cli / wire_weaver_py / ww_self: batched streams in the dynamic client and
  Python (API-41, piece 6 of `prj/P2401-wireweaver/design/batched_streams.md`): `DynPositionedStream` and
  `DynStream` decode a batched stream's events into items with their own device time, no generated code involved
  (`ApiItemKind::StreamFn` gains `batch_item_ty`, the `Timed<T>` instance the device's wire item needs);
  `recv_batch` / `recv_batch_blocking` give whole batches on both. `ww stream <path> [--batches]` prints a plain,
  counted or reliable stream the same way `ww observe` prints property updates. Python's
  `PositionedStream.recv_batch()` / `Stream.recv_batch()` give a whole batch as numpy columns
  (`ww_self::SeriesColumn::from_items`, built from the items, not a re-read of the batch's bytes). See the
  `wire_weaver_client`, `wire_weaver_cli`, `wire_weaver_py` and `ww_self` changelogs.
- wire_weaver_client: one device-time rule (P2401#17oct-2214b): every time a device sends (stream heads, batch items,
  observe updates) comes unwrapped into a `U64` in its prefix through `ww_si::Unwrapper` (now usable without an
  anchor), whatever prefix and storage the device picks; heads of counted and reliable streams were not unwrapped
  before. See the `wire_weaver_client` and `ww_si` changelogs.
- ww_si: exact integer time arithmetic (P2401#17oct-2214): `checked_add`, `checked_sub`, `checked_mul_int`,
  `wrapping_add`, `wrapping_sub` and `steps` on quantities of the same unit in integer storage; batched streams'
  generated `<name>_push_many(to, t0, dt, values)` stamps a block with `t0 + i * dt` through them instead of taking
  `(t, value)` pairs. See the `ww_si` and `wire_weaver_core` changelogs.
- `examples_mcu/mcu_qemu` (Cortex-M3, `lm3s6965evb`) and a new `examples_mcu/mcu_qemu_m4f` (Cortex-M4F,
  `mps2-an386`) gain a `Series<Timed<Ampere>>` encode benchmark (TEST-23, piece 4 of the batched-streams design):
  the motor demo's real recorded `i_a` signal, at n = 8/32/64 under `CodecPolicy::SMALLEST`,
  `fixed(delta,packed)` and `fixed(delta-of-delta,packed)`, one bin per codec set so `cargo size` /
  `llvm-size` measures each set's own flash. Flash: ~98.0 KB (M3) / ~75.7 KB (M4F) `.text`, within 70 B across
  codec sets (the trial-every-codec code for `SMALLEST` is a runtime branch, not a monomorphization axis, so it
  stays linked in regardless of which policy a bin uses); against a `ww_si`-free baseline bin (`qemu-bounded`,
  28.9 KB) linking `ww_si`'s `Series`/`NumericValue` codecs once costs about 69 KB. Instruction counts (semihosting
  `SYS_ELAPSED` measures host wall-clock time, not instructions, regardless of `-icount`; counted instead from a
  `qemu-system-arm -accel tcg,one-insn-per-tb=on -d exec,nochain` trace, bracketed by two marker functions): a
  fixed codec is 3.3x-3.9x cheaper than `SMALLEST` in CPU (not the ~10x first guessed), `fixed(delta,packed)` and
  `fixed(delta-of-delta,packed)` cost about the same. At the full FOC rate (21 properties x 25 kHz), even the
  fixed codecs need about 1 billion instructions/second on Cortex-M3, well past any M3/M4/M4F/M7 budget — full-rate,
  all-properties batched observe needs a property subset or a lower rate on real hardware, not just a cheaper
  codec. D4 (no_std default): `fixed(delta,packed)`, pending confirmation from DWT cycle counts on an
  STM32H725IG/M7 (follow-up on omarchy-m1). See `prj/P2401-wireweaver/design/batched_streams.md` for the numbers.
- shrink_wrap: `RefVecBuilder<T>` (SW-61) writes a list in place, one element at a time, with the bytes of the same
  `RefVec::Slice`. **Breaking:** it replaces `RefVecU8Builder` (`RefVecBuilder::<u8>::write_bytes_with`); see
  `shrink_wrap/CHANGELOG.md`.
- shrink_wrap: tuple and unit structs in `#[derive_shrink_wrap]` (SW-59), the same bytes as the struct with its
  fields named, in every layout; see `shrink_wrap/CHANGELOG.md`.
- Docs: the "Features" page is rewritten for newcomers (DOC-9). It opens with what WireWeaver is, then goes
  area by area: a sentence on what the area is for, its features grouped Basic, Advanced, Nomicon, each a short line
  on what it gives you with a link to its docs page, and features that work only in part marked. Still generated:
  the lines are kept in `tests/features_page/entries.md`, joined with the status in `FEATURES.md`, and `just test`
  fails when a done or in-progress feature has no line there.
- Docs: reading levels for the rest of the site (DOC-8). Every page of API, Evolution, Standard library, Transport,
  Examples and Dev tools has a Basic, Advanced or Nomicon badge, and each section lists its Basic pages first.
  New entry pages: API "Getting started" (a first API end to end), a standard library overview grouped by family
  (I/O, device contract, data types, framework) and a transport overview (which transport for what). Long pages are
  split: streams (counted streams, credit sinks, stream protocol), traits (trait attachments), crate layers out of
  the evolution rules, the USB internals, the framer wire format, `ww_dfu` (A/B and one-slot scenarios,
  internals), `ww_can_bus` (reference), the CLI (command reference) and Python (streams, observing, offline). Stale
  facts fixed on the way (USB connect example, framer checksum scope, missing `ww` commands, the GUI tool not
  building), and the three broken anchors in `ww_i2c.md`.
- Docs: reading levels (DOC-7). Pages carry a Basic, Advanced or Nomicon badge, and the SerDes section is
  regrouped by level: a short Basic path (shrink_wrap, derive, built-in types) followed by Advanced pages (size
  assumptions, derive reference, special types, fixed and sparse layouts, showcase, use cases, C) and Nomicon pages
  (wire format internals, code size). The built-in types page is rewritten against the code. A "What works today"
  page is generated from `FEATURES.md` (`just features-page`), which is now the "Feature tracker", last in the nav.
- `ww_recorder` (STD-34), an API to control a recorder (the SD card log store of P2628, repo `logring`, or any
  logger): `Recorder` with status, card facts, two-step format, recordings, markers, settings (flush thresholds) and
  counters, and a `Track[]` array with per-track enable, durability and counters. Docs page
  `docs/std_library/ww_recorder.md`; the first server is the logring's.
- `ww_block` (STD-39) and `ww_sd` (STD-40), the two generic card tiers of P2628. `ww_block::BlockDevice` reads,
  CRCs, erases and writes an SD card, eMMC or flash by block, writes only under a lease that stops the card's own user
  first, with an optional bulk credit sink and read stream. `ww_sd::SdCard` gives raw registers, an init report and
  single raw commands with response, status bits and timing; commands that can lock or damage a card are refused
  unless the firmware builds with `raw-sd-unrestricted`. Docs pages `docs/std_library/ww_block.md` and `ww_sd.md`.
- Motor controller demo (EX-11), `motor_demo` and `motor_demo_api` (code in the
  [motor_sim](https://github.com/vhrdtech/motor_sim) repo since 18 Oct 2026): a VESC-like FOC controller driving a
  simulated motor ([motor_sim](https://github.com/vhrdtech/motor_sim)), its 52 variables observed in three rate
  tiers through one trait level each, on simulated time faster or slower than real time, in-process or over UDP. The
  `motor_demo` CLI shows achieved rates, bytes against the budget, missed sends and the comparison with one
  `StreamData` per value; measured numbers in `docs/examples/motor_demo.md`.
- Sparse structs (DER-13): `#[derive_shrink_wrap(sparse)]` on a struct of `Option` fields writes only the present
  ones, as an `OptionAnyVec`, at slots given by declaration order or `#[slot = N]`; the base of the per trait level
  update type of observe requests (SRV-18). shrink_wrap/CHANGELOG.md, section Unreleased.
- Observed properties on the device (SRV-18): servers of APIs with `+observe` properties answer `Observe` requests
  and send one `PropertyUpdate` per trait level from the new `poll_properties`, within each level's budget, earliest
  deadline first, missed sends counted; the requests are validated and clamped as untrusted input. See the
  `wire_weaver_core`, `ww_stream` and `wire_weaver` changelogs, `docs/api/properties.md`.
- Observed properties on the host (CLT-28): generated clients have `observe(cfg)` per trait level with `+observe`
  properties and `observe_<name>(mode)` per property, typed settings, a latest-value cache, one device subscription
  per level shared by all its observers, sent again after a reconnect; together with SRV-18 this completes
  observe requests (API-38). See the `wire_weaver_client` and `wire_weaver_core` changelogs,
  `docs/api/properties.md#observing`.
- `wire_weaver_client`'s multi-request API is generalized (CLT-15): `MultiRead`/`MultiCall`/`MultiWrite` work for
  tuples of 2 to 8 resources at any depth (previously `MultiRead` only, 2-tuple, root level only), plus a new
  array-wide API (`read_array`/`call_array`/`write_array`) for the same resource across many indices
  (`Range`/`List`/`Mask32`), and a dynamic-client (`DynResource`) equivalent.
- Dynamic observe (TOOL-11): `ww_self` carries the `sparse` flag a dynamic decoder needs to read an
  `OptionAnyVec`-encoded struct (DER-13); `DynResource::observe()` decodes a trait level's `PropertyUpdate` from
  introspection alone, reusing CLT-28's event-loop routing and subscription sharing; `ww observe <path> [--rate MS]`
  prints each update's values, rate and link bytes/s; `wire_weaver_py`'s `Node.observe(cfg)` iterates the same
  updates as dicts. See the `shrink_wrap`, `ww_self`, `wire_weaver_core`, `wire_weaver_client`, `wire_weaver_cli`
  and `wire_weaver_py` changelogs, `tests/observe`, `docs/api/properties.md#observing`.
- Settings store for devices (STD-32): `ww_settings` keeps a struct of settings with defaults in NOR flash, as a
  record of the fields not at their default that every firmware version can read and write back without losing the
  others' fields, power-cut safe at every word written and byte erased; served through `value_on_changed`
  properties. See `ww_settings/ww_settings/CHANGELOG.md`, `docs/api/settings.md`.
- `ww settings dump` (TOOL-12): a host decoder of a `ww_settings::Store` flash dump, no device needed — pages,
  every item slot (valid, cut, garbage, erased) and the current record of every key, cross-checked against a real
  `Store` over the same bytes. See `ww_settings/CHANGELOG.md` (`dump` feature) and `wire_weaver_cli/CHANGELOG.md`.
- `examples/settings_api` + `examples/settings` (EX-15): two `value_on_changed` properties persisted through
  `ww_settings` on a simulated flash, over the real client/server codepath (`ww_device::Server` +
  `wire_weaver_client`'s `in_process` transport, a simulator use it explicitly supports). `tests/reboot.rs` sets
  both over a connected client, waits past their debounce, and rebuilds the device from the same flash image to
  check they survived. `UserSettings` (the `ww_settings::Settings` group) lives in `settings_api` so every
  implementation keeps the same record shape.
- `examples_mcu/nucleo_g0b1re`'s `settings` bin (EX-15): the same `SettingsDemo` API over RTT, backed by a real
  `ww_settings::Store` in the last 4 pages of bank 2 (`Bank2Region<Async>`, `memory.x` shrinks `FLASH` to keep
  code out of that range); `led_on_at_boot` also drives the board's user LED live. +21016 B flash, +1332 B RAM
  (`.bss`) over `ww_rtt` (release, `llvm-size`).
- `examples_mcu/nucleo_h743zi2`'s `settings` bin (EX-15): the same API over USB (same transport as `blinky`),
  backed by a real `ww_settings::Store` in the last 2 sectors of bank 2 (`memory.x` shrinks `FLASH` the same
  way). +23476 B flash, +2136 B RAM (`.bss`) over `blinky` (release, `llvm-size`). Both MCU bins are behind a new
  `settings` cargo feature (`dep:settings_api`, `dep:ww_settings`), checked by `just check-examples-mcu`.
- `shrink_wrap::ArrayString<N>` / `ArrayVec<T, N>` (SW-57): owned, no-alloc string/list types with the same wire
  bytes as `&str`/`String` and `Vec<T>`, for owned no-alloc fields (e.g. settings values, `value_on_changed` of a
  string/list property). shrink_wrap/CHANGELOG.md, section Unreleased.
- shrink_wrap: `SeriesStore` and series size bounds (SW-62), for batched streams (API-41): a device keeps a batch
  as a struct of arrays (`Timed<Ampere>` in 12 bytes per item where the items take 24), derived with `series`, by
  hand for `ww_numeric::NumericValue`; `Series::max_bytes(n)` bounds a batch before it is written. A `series` type
  nesting a type without a store needs `series(no_store)`; see `shrink_wrap/CHANGELOG.md`.
- ww_stream: the device side of batched streams (API-41), `ww_stream::batch`: a `Batcher` collects items in two
  struct-of-arrays stores shared by a producer (a control loop, an interrupt) and the event loop through one
  atomic word, no lock: a push copies the item and never encodes or waits, a batch is sealed on size, age or event
  budget, and the event loop encodes it once as a `Series` while the producer fills the other store; an `Outbox` for observed
  properties; batched reliable and counted streams. The `batched` API attribute, codegen and the host side are
  next; see `ww_stream/CHANGELOG.md`.
- Faster and smaller `Series<T>` writing on MCUs (SW-63), byte for byte the same output: about 6x fewer
  instructions per item on a Cortex-M3 (fixed codecs and `SMALLEST` alike), and compile-time codec policies
  (`series::DeltaPacked`, the `no_std` default of `ww_stream`'s batcher) that link only their own codecs: 15 KB of
  flash instead of 91 KB for a `Series<Timed<Ampere>>` encoder. `examples_mcu/qemu_icount` counts the instructions
  between the marker calls of the `mcu_qemu` benches and shows where they go (`--profile`, `--pcs`).
- Observing every sample (API-39): a `ro+observe+batched` property observed in `ObserveMode::Samples` sends every
  sample the device takes, each with its own time, as a compressed `Series<Timed<T>>` in its `Batched<T>` slot;
  generated O(1) `<Level>Sampler::sample(t, ..)` (also from an interrupt), batches encoded in `poll_properties`,
  typed and dynamic observers per sample, `ww observe --samples`, Python `recv_batch()`. Wire: API hashes of APIs
  with readable properties change. See the `wire_weaver_core`, `ww_client_server`, `ww_stream`,
  `wire_weaver_client`, `ww_self` changelogs and `docs/api/observing.md`.

### Changed

- `ww` shows struct values with their name when the type is known (`Version { major: 0, .. }`), so a value printed
  by `ww introspect` or `ww observe` reads back as a Rust expression (TOOL-9).
- The motor controller demo (EX-11) moved out of `examples/` into the [motor_sim](https://github.com/vhrdtech/motor_sim)
  repo with its history (`cd motor_sim && cargo run -p motor_demo`, against a `../wire_weaver` checkout), so it
  can grow into a full simulator with a GUI without those dependencies in this workspace. Its docs page stays here
  (`docs/examples/motor_demo.md`); `motor_demo_api` and the `motor_sim` git dependency left `[workspace.dependencies]`.
  `tests/observe` gains what the demo's tests covered beyond it: two levels observed at once under their own budgets
  (one at the device's default), and values due together costing less in one update than one update each.
- shrink_wrap: a unit struct with `sized`, `final_structure`, `self_describing` or `fixed_layout` is a compile
  error (zero bits that could never change); the docs explain that a plain unit struct records its length inside a
  parent and can grow fields later.
- Docs: reading levels adjusted. `EitherAnyVec`, `OptionAnyVec`, `TailBytes` and `TailSize` are Nomicon on the
  Features page; counted and reliable streams, stream stores, credit sinks and trait attachments are Nomicon in the
  API section and on the Features page; in Introspection, moved flag bits are Advanced and bundles Nomicon; the
  standard library's buses (UART, CAN, I2C, SPI, 1-Wire) are all Basic.
- New logos for wire_weaver (a twisted pair with the wordmark, a W icon) and shrink_wrap (a heatshrink sleeve on
  the pair, a cross-section icon), light and dark, in `docs/assets/`; READMEs link them by absolute GitHub URL so the
  crates.io pages show them; the docs site uses them and the small W as favicon
- Docs site in the VHRD brand look: red accent instead of the stock blue, IBM Plex Sans and Mono with Plex Serif
  headings, text-coloured links that turn red on hover, the brand dark palette, the small W in the header
  (`docs/stylesheets/vhrd.css`, colours from vhrd_brand 0.5.0)
- Docs home page: the logo, one short description and the buttons; the separate serif "WireWeaver" title is
  hidden (kept for screen readers and search), the wordmark in the logo is the title. Favicon
  `wire_weaver_favicon.svg` turns its black wire graphite on a dark browser theme, so it no longer shows only the red
  wire
- Docs site: the mobile nav drawer's logo is the graphite-wire W on dark too, like the header (only the red wire
  showed)

### Removed

- shrink_wrap / ww_self / wire_weaver_core / wire_weaver_client / wire_weaver_py / ww_logic_analyzer: the
  `Delta`, `DeltaOfDelta` and `XorFloat` sequence types (SW-31, SW-32); `Series<T>` covers them with the same codes
  plus automatic codec choice, and dynamic clients and Python now write it too. See the per-crate changelogs.

### Fixed

- Reliable streams (API-47): a resend or a resume from inside a multi-unit event (a batch confirmed item by item)
  gets that event again whole and the host keeps only the units it doesn't have; `ResendFrom` no longer frees store
  data an explicit host hadn't confirmed; a late `ResendFrom` no longer stalls a backfill. **Breaking** for code
  matching `ww_stream::rx::Verdict::Accept` (new `skip`) and for custom `StreamStore`s (`read` returns the event
  that holds a position); see `ww_stream/CHANGELOG.md`.
- ww_device: RTT devices panicked on their first request since TR-12 (an embassy-time `Timer` polled with a no-op
  waker); the async server now polls with the task's own waker (TR-13).
- Observed properties short of budget (SRV-18) no longer send one value per update: a level holds until all its due
  properties fit or its bucket is full, and a property held back counts as missed only once it really falls behind;
  `PropertyUpdate.t` is a wrapping `u32` of microseconds (4 bytes less), unwrapped by the host. Found with the motor
  demo (EX-11). See the `ww_stream` and `wire_weaver_client` changelogs.
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
