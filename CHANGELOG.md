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

- w0, the design line (EVO-41): a new API starts on `w0.0`, where breaks need no new line, no snapshots are kept, `#[since]` is refused and a client checks the device's introspection data instead of trusting a revision; the first release moves to `w1.0`. See `wire_weaver_evolution/`, `wire_weaver_core/`, `wire_weaver_client/`, `wire_weaver_cli/` and `ww_stdlib/ww_version/` CHANGELOGs.
- Evolution checker findings on a new wire line (EVO-33): the root's own unchanged traits are not breaks of their own, and every difference of a field is reported, scale and storage together. See `wire_weaver_evolution/CHANGELOG.md`.
- The C requantize rounds floats exactly, as `exact_units` does since NUM-30: one rounding from the float's bits, not a product in doubles and a cast; new corpus cases at the `f32` edge. See `shrink_wrap/c/` CHANGELOG.
- The evolution checker derives both bumps (EVO-27): `ww api check` says which wire version (`w2.3` -> `w2.4` for an addition, `w3.0` for a wire break, none for a Rust-only break) and which crate version a change needs, and fails when either is not bumped enough or a `#[since]` is newer than the wire version. See `wire_weaver_evolution/` and `wire_weaver_cli/` CHANGELOGs.
- Frozen hello in link setup (EVO-23): a few plain bytes at fixed positions open every link, with one generation byte each for the framer, link, shrink_wrap and API model, so a host and a device that can't talk name the layer and who should update instead of failing on a decode error or a timeout. `GetDeviceInfo` became `Hello` (wire break). See `ww_link/`, `ww_device/`, `wire_weaver_client/`, `shrink_wrap/`, `ww_framer/` and `ww_stdlib/ww_client_server/` CHANGELOGs.
- Arguments that evolve (EVO-22): methods and stream functions take trailing arguments with a default (`#[default = ..]`, implicit for `Option`, `Vec` and `String`), and an argument list can become a struct with `#[flatten]` without changing a byte. `ww api check` compares arguments like trailing struct fields and says when only Rust callers break. Breaking for introspection data and snapshots (`ww_self::Argument` gained two fields). See `ww_self/`, `wire_weaver_core/`, `wire_weaver_derive/`, `wire_weaver_evolution/`, `wire_weaver_client/`, `wire_weaver_py/`, `wire_weaver_cli/` and `wire_weaver_snapshots/` CHANGELOGs.
- The no-float firmware guard (TEST-33) is a gate in `just test` (`just no-float-guard`): `ww_si`'s device operations link no `f64`, 128-bit or slow-path code, and no `f32` where a firmware didn't opt into floats. See `ww_si/` CHANGELOG.
- `exact_units` taken at b114829 (STD-99, LVL-3): levels and their rates as quantities (`Neper`, `Decibel`, a loss
  base so a cable's dB/100 m never flips sign, damping, Beer-Lambert, dB/(cm·MHz) for ultrasound), reachable as
  `ww_si::..` through the existing `units` re-export; the crate walker learns the new `unit!` syntax
  (`counted(Base)`, `ln2`/`ln10` exponents) and `exact_units`' `Phase<A>` marker (STD-100, found on the way). See
  `wire_weaver_core/`, `ww_self/`, `ww_si/` and `wire_weaver_snapshots/` CHANGELOGs.
- Recording a board rework from any host (STD-98, TOOL-28): `ww_board_info` reworks are numbers (`ReworkId(u16)`, per board revision), the optional `BoardInfo::set_reworks` records them (all or nothing, dry run, refuses a number the firmware doesn't know), `REWORKS` lists an applied number the firmware doesn't know with `title: None`, and `ww_board_info::table` answers both from a firmware's own table. `ww board rework set N...` / `clear N...` call it on any device, `--dry-run` for a check, an unknown number shown with the numbers the firmware knows. Breaking for `ww_board_info` 0.3.0 (never published): devices implementing `BoardInfo` add `"board.set_reworks"` to `omit` or implement it. See `ww_stdlib/ww_board_info/CHANGELOG.md`, `ww_report/CHANGELOG.md`, `wire_weaver_cli/CHANGELOG.md`.
- WireWeaver syntax highlighting in the docs site (DOC-11): `docs/javascripts/ww_highlight.js`, run client-side on every page through Zensical's `document$` observable, colours `property!`/`method!`/`stream!` and their `ro`/`rw`/`wo`/`observe` access modes, WireWeaver attributes (`#[ww_trait]`, `#[derive_shrink_wrap(..)]`, `#[errors(..)]`, `#[optional]`, `#[default = ..]`, `#[flag]`, `#[slot]`, `#[future_version]`, `#[wire(..)]`) and exact_units' unit, prefix, origin and wrapper names inside the already-Pygments-highlighted ` ```rust ` blocks. No Pygments plugin, no Python at build time: Zensical's own highlighting stays untouched. Token lists live in `docs/javascripts/ww_tokens.json`, checked against exact_units' `src/units.rs`/`src/fixed/prefix.rs`/`src/point/origins.rs` and reusable by `ww_lsp` and an editor grammar. Colours reuse the theme's `--md-code-hl-*-color` variables, so light and dark palettes both work without new fixed colours.
- `exact_units` taken at 8584aad (STD-84): `Minute`, `Hour`, `Day`, `Week` and the `UnixEpoch` origin, with `Secs`, `Minutes`, `Hours`, `Days` and `UnixSecs`/`UnixMillis`/`UnixMicros`/`UnixNanos` reachable as `ww_si::..`. See `ww_si/CHANGELOG.md`.
- `exact_units` taken at d28ec86 (STD-85): `Count<Of, T>`, `Percent`/`Permille`, `Kibibytes`/`Mebibytes`/`Gibibytes`, `Hz`, and counted `Line`/`Row`/`Record`/`Entry` beside `Item`/`Sample`/`Frame`/`Packet`, all reachable as `ww_si::..`. See `ww_si/CHANGELOG.md`.
- Kinds of quantity that share a dimension (STD-68, first piece): a voltage gain V/V, a current gain A/A, a power gain W/W and a duty cycle no longer convert into each other, nor a transimpedance V/A into Ω (exact_units `Unit::KIND`, checked at compile time). The kind is in the API model (`MarkerDef::KindUnit` for `Ohm`, `Siemens`; a quotient's kind from the same rules), the evolution check compares it, a conformance case covers it; gains have dB by their kind (20·lg or 10·lg, first piece of STD-67). See exact_units', `ww_self/`, `wire_weaver_core/` and `wire_weaver_evolution/` CHANGELOGs.
- `#[wire(W)] field: T` in `derive_shrink_wrap` (DER-19): a plain integer field in Rust written as a narrower or variable-length wire type (`#[wire(I4)] offset: i8`, `#[wire(UNib32)] count: u64`), range-checked on write and read, also in `Series` columns and as `#[cfg_attr(pred, wire(W))]` for API crates with shrink_wrap optional. See `shrink_wrap/CHANGELOG.md`.
- Types from a crate that isn't a ww API crate (API-60): the walker walks and snapshots it, and generated code names its types through a re-export (`ww_si::exact_units::..`) when the generating crate doesn't depend on it; with `shrink_wrap` optional, its `#[cfg_attr(feature = .., derive_shrink_wrap(..))]` items are read with the features Cargo builds it with. See `wire_weaver_core/CHANGELOG.md`.
- Clock sync fits know how wrong their rate can be at most (`Fit::rate_bound_ppb`); the `io_timebase` tests no longer fail on a loaded PC. See `wire_weaver_client/CHANGELOG.md`.
- Live link handover to the ww hub on Linux (CLT-44): a program holding a USB device directly hands its open link to the hub when another program wants the device, instead of releasing it: the hub goes on with the same link, the device sees no reconnect, and the program goes on through the hub after a pause of a few milliseconds with its calls in flight answered and its streams going on. Rolls back when the hub dies before it holds the device; other OSes and media keep CLT-43's release and reopen. See `wire_weaver_client`, `ww_hub`, `ww_hub_api`, `ww_framer`, `ww_local_socket` changelogs.
- Polite handover to the ww hub (CLT-43): a command-line tool, notebook or test holding a device directly hands it to the hub when another program wants it (`ClientConfig::allow_handover`, on by default for those kinds), and both go on through the hub; the holder reconnects by itself after a gap of a few milliseconds (release and reopen; on Linux a live handover, CLT-44). A GUI or service says no, and the other program's error names it. Simulated single-host devices on a local socket take part with `ClientConfig::ipc_exclusive()`. See `wire_weaver_client/CHANGELOG.md`, `ww_local_socket/CHANGELOG.md`.
- Reconnect after a hub restart (CLT-40): a program reaching its device through the ww hub connects again by itself when the hub exits, crashes, is killed, hands over or lends the device, starting a hub if needed; counted and reliable streams, observed levels and sinks resume, calls in the gap fail with `Disconnected` and are never sent again, `Commander::connection_events()` tells the app. Calls made while no event loop runs fail at once (CLT-36). See `wire_weaver_client/CHANGELOG.md`.
- Hub version skew and handover (SRV-30): clients say their `ww_link` and `ww_client_server` versions in the preamble (a skippable trailer), the hub serves every compatible version and refuses others naming both sides and the fix; a program whose own hub is newer than the running one has it hand over, only strictly newer replaces older. See `ww_hub/CHANGELOG.md`, `ww_local_socket/CHANGELOG.md`.
- Device-wide exclusive access and lending through the ww hub (SRV-32): a claim on a device's root takes it whole for one program (other programs' calls and writes refused naming it, clock steering paused), which `ww dfu` does by itself; `ww hub lend DEVICE -- CMD` has the hub close a device for probe-rs, dfu-util or a terminal and opens it again after. See `ww_hub`, `ww_hub_api`, `wire_weaver_client`, `wire_weaver_cli` changelogs.
- The ww hub from Python and notebooks (TOOL-18): the `wire_weaver_py` wheel carries the hub, `python -m wire_weaver.hub` serves it (found by `connect()`'s hub routing the same way as a Rust `ww` executable), `connect(hub_device=...)` reaches a device through the hub, `wire_weaver.hub()` connects to the hub's own API through the dynamic client. Fixed on the way: `connect_blocking()` (used by Python, blocking test helpers) never reached a local socket or the hub (a missing `InterfaceKind::Ipc` branch).
- Clock sync in the ww hub (SRV-31): the hub steers every open device serving `ww_timebase::Clock` with host sync onto the PC's UTC and keeps it open for a sync linger (30 min) between short commands; `ww_hub_api::ClockSync` (devices, on/off, linger, stats stream); a per-device rate cache shared with direct-mode programs, so a known device is steered after its first reading; steer points name their steerer and a device follows one at a time (`ww_timebase::Steerer`); `ww sync` shows the hub's steering when a hub runs. Details in `ww_hub/`, `ww_hub_api/`, `ww_clock_sync/`, `ww_stdlib/ww_timebase/` and `wire_weaver_cli/` CHANGELOGs.
- Hub routing in `connect()` (CLT-39, `wire_weaver_client` feature `hub`): direct by default, through the ww hub when it already holds the device or after repeated short connections (3 within 60 s, shell-launched calls weighted), `HubMode` `Auto` / `Prefer` / `Required` / `Direct` (`WW_HUB`); the hub is started by the client when needed (start lock, newest hub executable, readiness by polling its socket). Lock files show the holder's client metadata. `ww hub take`.
- `ww hub status | stop | who | keep` (TOOL-17): look at and steer the per-user hub, see who holds each device with or without a hub, `ww list` through a running hub; completions with live device keys. Details in `wire_weaver_cli/CHANGELOG.md`.
- The ww hub process and its API (SRV-28, crates `ww_hub`, `ww_hub_api`, `ww hub serve`): one per user, the devices plugged in and who holds them, the connected programs, events; device tunnels come next (SRV-29).
- Device tunnels in the ww hub (SRV-29): several programs use one device at once through the hub (`ClientConfig::hub_device(key)`), each with its own link and seqs; streams opened once and fanned out, observed properties merged, introspection cached, slow clients' stream data dropped and counted, devices opened on demand and closed after a linger. Throughput as direct; a small request and its answer take 10-25 % longer over USB (two more process wakes).
- Local socket transport (TR-16): a device served by another process on the same PC, over a user-only Unix socket or named pipe (new crate `ww_local_socket`, `wire_weaver_client` feature `local_socket`, `ClientConfig::ipc_path`); the base of the planned ww hub (SRV-13).
- Device holders registry (CLT-38, `wire_weaver_client::holders`): every program opening a USB interface or RTT
  probe claims an OS file lock on `<runtime dir>/ww/holders/<key>.lock` first, so a busy error can name who
  already holds the device (program, pid, since) instead of the usual generic USB/probe error, and on Linux a
  non-WireWeaver holder is found by scanning `/proc`. The lock-file-per-device shape is what the planned per-user
  hub (SRV-13) will use too, to know which devices it already holds.
- Typed API metadata (SRV-34, first part): `#[claim(exclusive | shared | observe)]`, `#[shares(pins = "..",
  peripherals = "..")]` and `#[label("..")]` on items and traits, checked when the API is loaded and carried in
  introspection as data (`ww_self::Meta`), shown by `ww api tree`. The stdlib IO traits are claimable
  (`ww_can_bus`, `ww_uart`, `ww_i2c`, `ww_spi`, `ww_gpio::Bank`, `ww_dfu`, `ww_pin_diag`). `docs/api/metadata.md`.
- Per-resource claims through the hub (SRV-34): a program claims a bus, port or the whole device (`claims` group of
  the hub's API), and the hub answers other programs' calls and writes there with `Common(InUse)` naming it, while
  their reads and streams keep going; a claim goes with its program (crash included) or at its ttl.
  `docs/transport/hub.md`.
- `ww dfu flash` (CLT-33): a whole update of a chunky device in one command, with `chunky_ww_host` from
  chunky_mcu_updater: an ELF or binary sealed for the device, a delta plan, pipelined steps with a progress bar,
  reboot into the trial, follow by serial number, the device's confirm or revert. `ww dfu status`, `confirm`,
  `revert`. `PreparedCall::send` → `PendingCall` in `wire_weaver_client` sends now and awaits the reply later.
  `ww_dfu::NodeInfo` carries `product_id` and `seal` (STD-12).
- Frame prefix and link setup token (TR-15, wire change of the link): `ww_framer` writes an optional per-frame
  prefix (`SeqPrefix`: 4-bit frame counter, 2-bit session epoch) and tells lost, duplicated, reordered and stale
  frames apart; checksums cover prefix and head too. The host picks a random 64-bit token per connection, the
  device echoes it and starts a new framer session on `GetDeviceInfo`, and the host drops everything before its
  reply, so nothing of a previous session is taken for the new one.
  Per medium (`ww_link::Framing`): USB without a checksum and without a prefix (a frame counter as an optional
  diagnostic, told by the interface protocol), UDP with the frame counter and a CRC-32 (the one of `ww_block`) on
  every message; `ww stats` counts lost, duplicated and reordered frames.
- `BoardInfo::REWORKS` (STD-24, `ww_board_info` 0.3.0): an optional list of the reworks of a bring-up board (`id`,
  `title`, `applied`), so a unit says which patches of its PCB revision it has; `ww info` and `ww report` print it.
- `ww pins test` / `ww pins list` (TOOL-15, crate `ww_pin_probe`): shorts to GND, to the supply and between points,
  and pulls, on any device serving `PinDiag` at any path; points by names, ids, ranges and globs, plan files with
  expectations (`--plan`, `--save`), every point released at the end.
- `ww_pin_diag` (STD-47): one generic trait for pin tests, `PinDiag { const POINTS; set; read; release_all }`, so
  one host tool finds shorts, pulls and floating pins on any board; the firmware keeps its board's rules and refuses
  with `NotCapable`, `Common(InUse)` or `Refused { rule }`. `ww_interned` (STD-48): `Interned { Id, Text }`, names
  and reasons sent as an index into the firmware's string table, resolved from its ELF on the host. Docs page
  `ww_pin_diag.md`, test pair `tests/pin_diag`.
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
- `ww_si` quantities in those expressions (TOOL-9): `q!(3300 mV)`, `q!(1.2e-4 N*m/(rad/s))`, `q!("20 °C")`, or a
  plain number in the unit itself. Library: `wire_weaver_client::expr` (the `ww_self::expr` reader with quantities,
  through its new `Hook`), `quantity::parse_quantity`.
- Whole messages explained (TOOL-9): `ww explain request HEX`, `ww explain event HEX [--for PATH]` and `--request`
  (the request a call, write or read is on the wire) show the `ww_client_server` envelope field by field and the
  payload inside it by its type. Library: `wire_weaver_client::envelope`, `DynResource::descend` (a resource from a
  request's absolute path).
- `explain` in the wire-format conformance matrix (TEST-23, TOOL-9): every corpus case's value written as a Rust
  expression and read back before it is encoded, and the golden bytes explained with every bit in exactly one part.
  For it `ww_self::explain` now shows padding at the end of values and before nested ones, a sparse struct's slot
  header and bytes skipped from a newer version as parts of their own, and `ww_self::expr::to_expr` writes a value
  as a Rust expression. Found and fixed on the way: `i128::MIN` didn't read back, generic structs printed with their
  arguments (`Timed<u16> { .. }`).
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
- `ww_board_info` (STD-24): `RevisionSource::Provisional`, a provisional board record written in flash by a bench or
  developer; `ww info` shows it as "provisional record".
- Host clock sync onto one UTC timescale (STD-49, STD-50, TOOL-16): `ww_timebase` ranks a device's sources (sync
  line > host > holdover > free), gets the optional `host_sync` group (`steer`, `shared_now`), `SyncSource` and the
  followed source and error in `SyncStatus` / `time_map`. New crate `ww_clock_sync` fits each device's clock from
  min-RTT `now()` readings and steers it onto the host's wall clock, for any process holding a connection; tested
  with simulated drifting and jittery device clocks. `ww_indication`: devices with a shared clock align their
  animations to period boundaries of UTC, so they breathe in phase. `ww sync DEV...` keeps several devices steered
  and shows how well they follow.
- The rework guard (STD-101, TOOL-29): a firmware never runs a board with a rework it doesn't know. First thing in `main`, before any init, it reads the bench record and refuses to boot unless an allowance bound to its own image's build id covers the reworks: it blinks a shared code and reboots into the ROM bootloader. `ww_board_info::guard` decides (`boot_check`), `ww_board_info::image` carries the reworks an image knows and where the board's record is (the locator), `ww_board_info::record` reads and writes the record through a boot ROM. `ww dfu rp flash` / `ww dfu stm32 flash` check there before writing and store or clear the allowance there. Replaces "runs on its safe features". See `ww_stdlib/ww_board_info/CHANGELOG.md`, `ww_settings/ww_settings/CHANGELOG.md`, `ww_report/CHANGELOG.md`, `wire_weaver_cli/CHANGELOG.md`, `ww_stdlib/ww_firmware_info/CHANGELOG.md`.
- Flashing checks the board's reworks (TOOL-29): `ww dfu rp flash` and `ww dfu stm32 flash` reboot a running device into its boot ROM themselves, read its applied reworks there through the ROM, compare them with the ones the new image knows, ask before writing an image that lacks one (or take `--allow-unknown-reworks`), and store or clear the allowance there. The check is a library, `ww_report::rework_check`. See `wire_weaver_cli/CHANGELOG.md`, `ww_report/CHANGELOG.md`.
- `ww info` and `ww board rework allow|disallow` work on a board stuck in its boot ROM (TOOL-30), reading and writing the bench record and the allowance straight through PICOBOOT / DfuSe: the verdict a reboot would reach right now, and a way to fix it, without needing the board to boot first. See `ww_report/CHANGELOG.md`, `wire_weaver_cli/CHANGELOG.md`.
- Large messages (CLT-55): the message limit comes from `ClientConfig::max_message_len`, both sides keep to the smaller of the host's and the device's, an over-limit call fails at once with `Error::MessageTooBig` naming both numbers, `no_alloc = false` servers reply past their scratch buffer, and each session keeps the host's API and app version. No wire change. See `docs/api/large_messages.md` and the `wire_weaver_client/`, `ww_device/`, `wire_weaver/` and `wire_weaver_core/` CHANGELOGs.
- `CallContext` (SRV-48): who asked (peer, trace, deadline, connection, priority) in every server `Context` (`cx.call()`) and as the first argument of an API's local trait, whose errors and streams now live in `wire_weaver::call`, so an API crate's `local` feature needs no `wire_weaver_client`. See `wire_weaver/`, `wire_weaver_core/` and `wire_weaver_client/` CHANGELOGs.
- Observe left out of firmware that doesn't observe (SRV-54): server code writes each event in place instead of serializing a whole `Event`, and `RequestKind::Observe`'s body is decoded only where a server observes. A one-method server on a Cortex-M0+ went from 26.3 KB to 14.5 KB of flash at `opt-level = "z"`; the wire is unchanged. Breaking for Rust code building `RequestKind::Observe` by hand (`cfg: RefBox::new(&cfg)`). See `ww_stdlib/ww_client_server/CHANGELOG.md`, `wire_weaver_core/CHANGELOG.md`, `docs/articles/flash_budget.md`.
- Golden reply bytes of the dispatcher (TEST-31), guarding the flash-diet dispatcher rework (SRV-51, SRV-52,
  SRV-53): `tests/dispatch_golden_api` and `tests/dispatch_golden`, a corpus of 25 request/reply cases covering
  every resource kind, checked against both a sync and an async server and frozen like the wire-format conformance
  corpus. See `tests/dispatch_golden/corpus.json` and `docs/serdes/conformance.md`.
- Simulated UDP for tests (TR-21): `wire_weaver_client::in_process::udp_device`, the UDP framing without the kernel, so tests on tokio's paused clock drive the frame packing exactly. `tests/dfu_scenarios` runs 1S-APP-BULK and 2C-APP-BULK on it, and their transcripts no longer change with the machine's load. See `wire_weaver_client/CHANGELOG.md`, `ww_tap/CHANGELOG.md`.
- Simulated WebSocket for tests (TR-22): `wire_weaver_client::in_process::ws_device`, the `ws` transport over an in-memory pipe, so tests on tokio's paused clock decide the order of both ends' frames. `tests/dfu_scenarios` runs 1L-UPD-BULK and 2R-APP-STEP on it. See `wire_weaver_client/CHANGELOG.md`, `ww_tap/CHANGELOG.md`.
- A crash's call chain as `file:line` (CLT-59): `ww report` decodes the crash record's addresses with the ELF's DWARF, inlined frames included, and marks the panic site, so firmware built with `-Zlocation-detail=none` stays debuggable; `ww_report::crash` for other host tools. See `ww_report/CHANGELOG.md`.
- Flash attribution by inlined frames (TEST-32): `fw_budget --attribute ELF [--gen GENERATED.rs]` (`just flash-attribute ELF`) splits a firmware's bytes by the crate of the innermost frame, by generated dispatcher function and by shrink_wrap serdes impl, for when a flash total moves and the reason isn't obvious. A development tool, no crate changes.
- No-float firmware guard (TEST-33): `just flash-budget` builds a fixture calling every device-relevant `ww_si::Quantity` operation of today's API on two cores, two firmware profiles and four number-feature sets, and lists the f32, f64 and 128-bit helper symbols, the VFP instructions on thumbv7em and the named slow paths each build links, with bytes per crate. Firmware without floats must link none of them, one with `f32` no f64 (`--guard-strict` fails the run). Baseline recorded in FEATURES.md, red until STD-102 moves `ww_si` onto exact_units' integer kernel. A development tool, no crate changes.
- Smaller decoders for enums mixing unit variants and variants with fields (SW-71), the same bytes: `exact_units::Prefix` from 624 to 324 B on a Cortex-M0+ at `opt-level = "z"`. See `shrink_wrap/` CHANGELOG.
- The generated dispatcher of a `no_alloc` server keeps only decode, call and a shared reply path per resource (SRV-51): 86 B instead of 238 B per resource on a Cortex-M0+ at `opt-level = "z"`, same reply bytes. `just dispatch-budget` counts instructions per request (TEST-30). See `wire_weaver_core/` and `ww_client_server/` CHANGELOGs.
- The generated dispatcher writes errors where they happen instead of passing a `Result` up the levels (SRV-52): a further 316-600 B off the TEST-30 server, and 6-8 % fewer instructions per request at `opt-level = "z"` than before SRV-51. See `wire_weaver_core/` and `ww_client_server/` CHANGELOGs.
- The dispatcher's reply paths have one copy for every type instead of one per value type, and `MaxLen` one decoder per element type (SRV-55): Donguru's firmware 17.2 KB smaller, below its size before SRV-51; instructions per request within 1.5 %. See `ww_client_server/`, `shrink_wrap/` and `ww_device/` CHANGELOGs.
- Global trait paths without names in flash (SRV-53): a device with introspection carries no global trait lookup, the client resolves `GlobalFull` and `GlobalCompact` paths from the introspection data and sends the absolute path (on a `traits_only` connection after the device's `TraitNotFound`, fetching the data once); a device without introspection finds a `GlobalFull` trait by a 32-bit FNV-1a hash of crate and trait name in a const table, a collision is a compile error. `ww_codegen!(.., global_paths = "full" | "compact" | "none")` chooses otherwise. Servers with introspection are 120-156 B smaller on a Cortex-M0+, one more global-path trait without it 80 B instead of 120 B; Donguru's firmware 2,416 B smaller, donguru_template's 1,868 B. Breaking for `wire_weaver_core`'s `GenServerConfig` (a `global_paths` field) and for hosts with their own client talking to a device with introspection by global paths. See `ww_global/`, `ww_stdlib/ww_client_server/`, `wire_weaver_core/`, `wire_weaver_derive/` and `wire_weaver_client/` CHANGELOGs.
- `ww_codegen!(.., observe_samples = false)` (SRV-56): a firmware that takes no samples of its `+batched` properties links no batcher, outbox or `Series` encoder, with the same API and wire; `Samples` is refused with `ModeNotSupported`. wire_weaver_core, wire_weaver_derive, wire_weaver_cli changelogs.
- `ImageFacts` in the link's `DeviceInfo` (SRV-56): a firmware image says what it chose at build time, outside the API hash, and clients refuse `Samples` up front on an image that takes none. `LinkConfig::new` takes the generated `image_facts()`, a Rust break and no wire break. ww_version, ww_link, ww_device, wire_weaver_core, wire_weaver_client, ww_hub changelogs.
- Compressed introspection data as an option (INT-16): `ww_codegen!(.., introspect_compression = "lz4")` keeps a device's `WW_SELF_BYTES` as LZ4 blocks made at build time (no decoder on the device, no code cost), sent like a plain bundle; clients tell the formats apart by the first bytes (`WWz4` or `ww_self::MAGIC`), decompress on every path and refuse an unknown magic. API hashes and the cache stay those of the uncompressed bundle. rePort -4,312 B of flash, Donguru -920 B. See `ww_self/`, `wire_weaver_core/`, `wire_weaver_derive/`, `wire_weaver_client/` and `wire_weaver_cli/` CHANGELOGs, `docs/api/introspection.md`.
- shrink_wrap's reader and writer plumbing in less flash (SW-74): `latched()` stores only an error it clears, a size-prefixed value is read through one out-of-line split and written without a `Result` copied per type, `BufReader::new` / `BufWriter::new` are out of line unless shrink_wrap's `fast-io` feature is on. Donguru −3,984 B at `opt-level = "z"` and −18,176 B at `3`, rePort −6,360 B; same bytes on the wire. See `shrink_wrap/CHANGELOG.md`.
- shrink_wrap's bit writer and reader are no longer forced inline (SW-70): about 0.75 KB less flash in a firmware built with `opt-level = "z"`, task-side series encoding 5-22 % slower there; the `fast-io` feature (on with `std`) restores the old code, `optimize-speed` (Rust 1.101 or nightly) is faster than before. Lists of plain numbers share one encode and skip path. See `shrink_wrap/CHANGELOG.md`.
- docs: internal references removed
- wire_weaver_client / wire_weaver_cli / wire_weaver_py / ww_self: batched streams in the dynamic client and
  Python (API-41, piece 6 of our internal design notes on batched streams): `DynPositionedStream` and
  `DynStream` decode a batched stream's events into items with their own device time, no generated code involved
  (`ApiItemKind::StreamFn` gains `batch_item_ty`, the `Timed<T>` instance the device's wire item needs);
  `recv_batch` / `recv_batch_blocking` give whole batches on both. `ww stream <path> [--batches]` prints a plain,
  counted or reliable stream the same way `ww observe` prints property updates. Python's
  `PositionedStream.recv_batch()` / `Stream.recv_batch()` give a whole batch as numpy columns
  (`ww_self::SeriesColumn::from_items`, built from the items, not a re-read of the batch's bytes). See the
  `wire_weaver_client`, `wire_weaver_cli`, `wire_weaver_py` and `ww_self` changelogs.
- wire_weaver_client: one device-time rule: every time a device sends (stream heads, batch items,
  observe updates) comes unwrapped into a `U64` in its prefix through `ww_si::Unwrapper` (now usable without an
  anchor), whatever prefix and storage the device picks; heads of counted and reliable streams were not unwrapped
  before. See the `wire_weaver_client` and `ww_si` changelogs.
- ww_si: exact integer time arithmetic: `checked_add`, `checked_sub`, `checked_mul_int`,
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
  STM32H725IG/M7 (follow-up on one of our dev PCs). See our internal design notes on batched streams for the numbers.
- `ww_recorder` (STD-34), an API to control a recorder (the SD card log store of repo `logring`, or any
  logger): `Recorder` with status, card facts, two-step format, recordings, markers, settings (flush thresholds) and
  counters, and a `Track[]` array with per-track enable, durability and counters. Docs page
  `docs/std_library/ww_recorder.md`; the first server is the logring's.
- `ww_block` (STD-39) and `ww_sd` (STD-40), the two generic card tiers behind `logring`. `ww_block::BlockDevice` reads,
  CRCs, erases and writes an SD card, eMMC or flash by block, writes only under a lease that stops the card's own user
  first, with an optional bulk credit sink and read stream. `ww_sd::SdCard` gives raw registers, an init report and
  single raw commands with response, status bits and timing; commands that can lock or damage a card are refused
  unless the firmware builds with `raw-sd-unrestricted`. Docs pages `docs/std_library/ww_block.md` and `ww_sd.md`.
- `just test --changed` runs only the workspace crates changed against main and their dependents (nextest's `rdeps()`); plain `just test` runs everything, as `just test-all` does.
- Wire versions apart from crate versions (EVO-24): an API root declares `#[ww_api_root(wire = "w2.3")]` (line 2, revision 3, always written with the `w`), the trait gets a `*_WIRE_VERSION` const and the bundle carries it, and `#[since = "w2.1"]` names the wire revision that added an item; a crate version in either place is refused with a hint. Every API root needs `wire = ..` now; link setup still decides on crate versions until EVO-25. See `ww_version/`, `ww_self/`, `wire_weaver_core/`, `wire_weaver_derive/`, `wire_weaver_client/`, `wire_weaver_evolution/`, `shrink_wrap/` and `wire_weaver_snapshots/` CHANGELOGs.
- The frozen hello on every medium (EVO-23): USB, UDP, RTT and WebSocket write and read it outside the framer, so a peer whose framer is of another generation is still understood; the hub preamble carries the client's generations and the hub serves by them. See the changelogs of `ww_link`, `ww_device`, `wire_weaver_client`, `ww_local_socket` and `ww_hub`.
- Wire versions of shared crates (EVO-26): a crate without an API root declares its wire version in `Cargo.toml` (`[package.metadata.wire_weaver] wire = "w1.2"`), bundles and snapshots carry it per crate, and `#[since]` in it names its revisions, compared with the device's wire version of that crate (`ww_board_info`'s `set_reworks` was refused under API roots at `w1.0`). Dependencies are compared by wire line where both sides declare one, so a Rust-only break of a `ww_stdlib` crate no longer breaks the APIs using it. Every `ww_stdlib` crate is at `w1.0`, `ww_board_info` at `w1.2`; snapshots saved again, signatures unchanged. See `ww_self/`, `wire_weaver_core/`, `wire_weaver_client/`, `wire_weaver_evolution/` and `ww_stdlib/*` CHANGELOGs.
- Link setup decides on the wire line, not the crate version (EVO-25): `DeviceInfo` and `LinkSetup` carry the API root's wire version, a device and a host of one API on the same line connect whatever their crate versions, another line is refused naming both; `LinkConfig::new` and `ClientConfig::client_version` take the wire version. See `ww_link/`, `ww_device/`, `wire_weaver_client/`, `wire_weaver_core/` and `ww_hub/` CHANGELOGs.
- Wire version in Cargo.toml (EVO-31): an API root crate declares its wire version like every other crate, `[package.metadata.wire_weaver] wire = "w2.3"` with a bare `#[ww_api_root]`; `#[ww_api_root(wire = ..)]` is kept for test crates (`publish = false`) and must agree with `Cargo.toml`. Every API root in the repository moved. See `wire_weaver_core/`, `wire_weaver_derive/` and `ww_hub_api/` CHANGELOGs.
- NaN-free floats in clients (STD-105): `ww`, Python and the dynamic client refuse NaN for a float `Fixed` unless its bound is `MaybeNan`, on writing and on reading; the walker, the evolution checker and the conformance corpus know `MaybeNan`. See `ww_self/`, `wire_weaver_core/`, `wire_weaver_evolution/`, `wire_weaver_client/`, `wire_weaver_py/`, `ww_si/` and `wire_weaver_snapshots/` CHANGELOGs.
- `exact_units` taken at db126d6 (NUM-19 nan-free-floats, NUM-22, NUM-23, NUM-24, NUM-28, PT-6, LVL-7): a float `Fixed` is never NaN unless its bound is `MaybeNan`, so a NaN that used to decode into one is refused; `Fixed::new` of a float doesn't compile (`try_new`, `bounded`). Breaking for `ww_si` users through its re-exports. See `ww_si/` CHANGELOG.
- USB vendor descriptor constants and the self-id-and-version byte encoding moved from `wire_weaver_usb_embassy` to `ww_link` (TR-23), so an embassy-free USB stack (`bedrock_usb`) can write the same bytes; `wire_weaver_usb_embassy`'s own output is unchanged. See `ww_link/CHANGELOG.md`, `mcu/wire_weaver_usb_embassy/CHANGELOG.md`.
- A host for several wire lines (EVO-28): one host driver bundles a generated client per wire line of an API and connects once to a device on any of them (`ClientConfig::speaks`, `client_line()`); codegen names an API crate taken under another dependency name by that name, and the checker reports a removed enum variant as breaking. Scripts reach every line too: Python makes quantities from text (`ww.Quantity("0.25 K")`) that convert exactly into each line's types, and has `dev.wire`. Bench `tests/thermo` with three lines, docs `evolution/multi_line_host.md`. See `wire_weaver_client/`, `wire_weaver_core/`, `wire_weaver_evolution/` and `wire_weaver_py/` CHANGELOGs.
- Several wire lines in one API crate (EVO-37): a `#[ww_api_root]` per module (`wire_v1`, `wire_v2`) with its wire version in `[package.metadata.wire_weaver.roots]`, the types the lines share in one module, `ww_codegen!(api::wire_v2::Thermo ..)`. Test `tests/multi_root`, docs `evolution/rules.md#several-lines-in-one-crate`. See `wire_weaver_core/` and `wire_weaver_derive/` CHANGELOGs.
- A host's write of an observed `get_set` property reaches its observers without the setter calling `mark_changed_<name>()` (SRV-57): the generated server marks it after `SetResult::Set`. See `wire_weaver_core/` CHANGELOG.
- Quantity specs in the model (exact_units UNIT-8, EVO-45, a wire change): ISO 80000's tree of quantities from `quantity!` lines, `Of<Q, U>` and units that name their quantity by id, read by the walker, carried in bundles and snapshots, compared by the evolution checker and the dynamic client. See the `ww_self`, `wire_weaver_core`, `wire_weaver_evolution`, `wire_weaver_snapshots` and `ww_si` CHANGELOGs.
- Snapshots per API root (EVO-38): `ww api save` on a crate with several wire lines saves a snapshot per root when it changed (`multi_root_api_0_1_0.wire_v2.Thermo@w2.1.ron`), each checked against that root's previous one, and warns about a root with no earlier snapshot. Docs `evolution/checker_tool.md#several-api-roots-in-one-crate`; the doc-comments row of `evolution/rules.md`'s bump table says what the compatible bump is for. See `wire_weaver_evolution/`, `wire_weaver_core/`, `wire_weaver_cli/` and `wire_weaver_snapshots/` CHANGELOGs.
- One client for a sub-trait the lines of a crate share (CLT-68): `ww_codegen!(ww_indication::Indication for IndicationClient, client = "sub_level")` once, and `sub_levels = [ww_indication::Indication => crate::IndicationClient]` on each line's client, which then returns it wherever it reaches the trait; reached by path, so the device is unchanged. A server whose API has one trait at two places compiles now. Test `tests/shared_sub_level`, docs `evolution/multi_line_host.md#shared-sub-traits`. See `wire_weaver_core/` and `wire_weaver_derive/` CHANGELOGs.
- `ww api scaffold --root <Trait>` (TOOL-32, `--name` kept as a hidden alias) picks the API root to scaffold by its module path in a crate with several roots (`--root wire_v2::Thermo`, EVO-37), and the printed `ww_codegen!` call and comments now name it the same way, so the output compiles. See `wire_weaver_core/` and `wire_weaver_cli/` CHANGELOGs.
- A device's hello and its own introspection bundle must agree on its wire version (CLT-61): a device reporting two
  different wire versions in the two places is refused before either is trusted, naming both. See `wire_weaver_client/` CHANGELOG.
- `WwClient::try_from_cmd` (CLT-62): the fallible form of `from_cmd`, so a facade of several wire lines (EVO-28) can say in its error why a line it has no client of got through link setup, rather than `unreachable!`. See `wire_weaver_client/CHANGELOG.md`.
- The checker per API root (EVO-39): `ww api check` and `ww api diff` of a crate with several wire lines check each root against its own latest snapshot, name a changed shared type with every line it changed on, ask the crate version of the root that needs the most, and treat a removed root as a breaking crate bump that `ww api save` records. One `CrateCache` serves every load of a check. Docs `evolution/checker_tool.md#checking-each-root`. See `wire_weaver_evolution/`, `wire_weaver_core/` and `wire_weaver_cli/` CHANGELOGs.
- A codec's spare codes in the model (EVO-49, exact_units at 4abc92b): `CodecDef::spare`, `MuLawSingleZero`, a spare code refused by every client and a `Niched` claiming it, new conformance cases. See `ww_self/`, `wire_weaver_core/`, `wire_weaver_client/`, `wire_weaver_py/` and `wire_weaver_snapshots/` CHANGELOGs.

### Changed

- Run-time units by reference (STD-107, CLT-66, EVO-36, exact_units UNIT-7): an `AnyQuantity`'s `SIExpr` names a unit the API lists by its crate and marker (`Ref`), resolved through the API model by clients, `ww`, Python and saved captures; two crates' `Rpm`s stay apart. See `ww_stdlib/ww_si`, `ww_self`, `wire_weaver_client`, `wire_weaver_py`, `wire_weaver_cli` and `ww_stdlib/ww_adc` CHANGELOGs.
- Unit identity in the API model (STD-106, exact_units UNIT-7): a unit's definition is `exact_units`' `UnitDef` with its crate and marker name, scales and origins carry ids, and the walker refuses two markers of one crate named alike; `MarkerDef` folds its three unit variants into one (wire break of the model, EVO-35). See `ww_self/`, `wire_weaver_core/`, `wire_weaver_evolution/`, `wire_weaver_client/`, `ww_si/` and `wire_weaver_snapshots/` CHANGELOGs.
- `ww_si` conversions, comparisons and bounds on `exact_units`' integer kernel (STD-102): one `to_in::<T>(prefix, Rounding, Overflow)` and `rescaled` instead of five methods with a policy in each name, exact for every prefix, no float or 128-bit code on a device; `exact_units` by git without a `rev`, `Cargo.lock` pins 1819d9f (the kernel's `u32` fast path). See `ww_si/` and `ww_numeric/` CHANGELOGs.
- `ww bootsel` and `ww stm32dfu` moved under `ww dfu` as `ww dfu rp` and `ww dfu stm32`; the old names are gone. See `wire_weaver_cli/CHANGELOG.md`.
- `ww sync` on a terminal shows one live view from the start (who steers, the table, how far the devices converged) instead of log lines above a redrawn table, and stops on `q` too, says who steers the devices after it stops (the ww hub, or nobody: holdover), and shows the phase spread between devices without assuming LEDs. See `wire_weaver_cli/CHANGELOG.md`.
- Denser `ww` help: a subcommand shows only the global flags it uses (device selection, link tap), and its `--help` sums them up in one line per group instead of spreading them over ~60 lines; `-h` and `ww --help` keep them in full. See `wire_weaver_cli/CHANGELOG.md`.
- Credit sinks on the stream protocol (API-34): the same `Ack`, `Resend` and `Query` as reliable streams, positions in units the host names per item (one per value, the payload's bytes on a byte sink, `send_units`), grants in bytes, sessions named by the host in `StreamOpen` so a repeated or late open no longer resets a running session and a device that lost its session is reported (`StreamError::SinkSessionLost`). State machines in `ww_stream::sink` (`SinkCredit` on the device, `SinkTx` on the host), a read on a sink returns its `StreamStatus`. Wire-breaking: the old `Credit` / `CreditQuery` / `Resend` sideband, `CreditWrite` and `ww_client_server::credit` are gone, `ResendFrom` is `Resend`, and `StreamSideband`'s discriminants moved, so devices and hosts with reliable or counted streams or credit sinks are rebuilt together. See the CHANGELOGs of `ww_client_server`, `ww_stream`, `wire_weaver_core`, `wire_weaver_client` and `wire_weaver_py`.
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
- nextest timeouts are hang detectors (`.config/nextest.toml`): SLOW after 15 s, killed after 30 s instead of 3 s, so a loaded PC (load well above the core count) can't fail the gate with a test that passes on a quiet one; the per-test 15 s overrides are gone, the default covers them. `AGENTS.md` "Test time limits" says how speed is looked at instead.
- Docs: `std_library/exact_units.md`, `magnitude.md`, `points.md`, `rounding.md`, `indices.md`, `customary.md` and
  `units_in_the_model.md` keep only the WireWeaver side of `exact_units` (the API model, `ww`, Python, the dynamic
  client, the wire bytes) and link to the crate's own book, [exact.vhrd.tech](https://exact.vhrd.tech/), for the
  library itself (scales, bounds, aliases, points, indices, rounding, customary units); nothing moved out without a
  page there to land on. `FEATURES.md`, `AGENTS.md` and `features_today.md` (`just features-page`) point the same
  way; `evolution/units_by_meaning.md`'s two deep links to the old pages now go to the book.
- Measured constants stay symbols (exact_units NUM-10, EVO-43): μ₀ and G are exact symbols of a `Magnitude` in the model and on the wire, beside π, ln 2 and ln 10; a scale such as an oersted's is exact, and a client converts it only through a named set of values. The walker reads `mu0 e` and `g e`. **Breaking:** the wire form of `Magnitude` changes (two more symbol exponents, 16 prime slots), so the conformance goldens of magnitudes, offsets, SIExpr and model markers carry the change. See `wire_weaver_core/`, `wire_weaver_client/`, `wire_weaver_py/` and `wire_weaver_snapshots/` CHANGELOGs.
- `exact_units` taken at 1e22ed0 (NUM-10, PT-4): besides measured symbols, the time scales `TaiScale`, `PosixScale` and `DeviceMonotonic` with the epochs `NtpEpoch`, `TaiEpoch1958`, `GpsEpoch`, `PtpEpoch` and `DeviceBoot` reach `ww_si::..` and its snapshot; `UnixEpoch` moves off `UnitZero` onto `PosixScale`. The model keeps an origin's reference by name only for now (STD-108).
- The dimension's shape and a unit's names (exact_units UNIT-10 and IO-9, EVO-44): a `Dim` holds seven exponents in halves and any number of distinguished bases (rad, bit, neper, counted things, a crate's own `base` unit), and a unit's definition carries its ASCII form, long names, aliases, UCUM code and prefixes; the walker reads the new `unit!` lines, `MarkerDef::Base` names a unit that is a base, Python's `dim` gives halves as floats, the corpus and snapshots move. See `wire_weaver_core/`, `ww_self/`, `ww_si/`, `wire_weaver_client/`, `wire_weaver_py/`, `wire_weaver_evolution/` and `wire_weaver_snapshots/` CHANGELOGs.
- Systems of units in the model (EVO-47, exact_units UNIT-9): a dimension names its system (SI, Natural, Gaussian), the walker reads `[Gaussian: ..]` dimensions, `MarkerDef::System` carries a system's definition, `ww_si` re-exports the systems. A wire change of every `Dim`. See `wire_weaver_core`, `ww_self`, `ww_si`, `ww_adc` and `wire_weaver_snapshots` changelogs.
- References of origins in the model (STD-108, EVO-48, exact_units PT-4): an origin carries what its zero is measured from and how that zero is known (exact, at run time, a time scale), the walker reads `impl Reference`, the evolution checker says why two time scales don't convert. A wire change of every `OriginDef`. See `ww_self`, `wire_weaver_core`, `wire_weaver_evolution` and `ww_si` CHANGELOGs.
- Non-linear codes in the model and the clients (STD-109, exact_units LVL-6): `MarkerDef::Codec`, the walker reads `impl Codec`, `ww`, Python and the dynamic client read a G.711 μ-law byte as the sample it means and write by its exact code; conformance cases with each byte's decoded value. See `ww_self`, `wire_weaver_core`, `wire_weaver_client` and `wire_weaver_py` CHANGELOGs.
- `Finite` floats in the model and the clients (STD-104, exact_units NUM-20): ±∞ and NaN refused by `ww`, Python and the dynamic client for a `Finite` float, the evolution checker compares the bound. See `ww_self` and `wire_weaver_evolution` CHANGELOGs.

### Removed

- shrink_wrap / ww_self / wire_weaver_core / wire_weaver_client / wire_weaver_py / ww_logic_analyzer: the
  `Delta`, `DeltaOfDelta` and `XorFloat` sequence types (SW-31, SW-32); `Series<T>` covers them with the same codes
  plus automatic codec choice, and dynamic clients and Python now write it too. See the per-crate changelogs.

### Fixed

- `#[since]` items stay callable after a breaking bump of the API (`0.5.2` -> `0.6.0`): the host compares the device's version with the marker in full instead of only the patch (only the minor after 1.0), and the evolution checker warns about markers newer than the crate. See `wire_weaver_client/CHANGELOG.md`, `wire_weaver_evolution/CHANGELOG.md`.
- `derive_shrink_wrap` reads `#[cfg_attr(pred, default = ..)]` (and its other field and variant attributes) on fields and variants instead of dropping them without a word, with a compile error where `pred` doesn't hold (API-60). See `shrink_wrap/CHANGELOG.md`.
- `derive_shrink_wrap` keeps the attributes it doesn't read on fields and variants (`#[serde(..)]`, `#[cfg_attr(..)]`) on the generated type with the item's name, instead of dropping them with an "Unknown attribute" line. See `shrink_wrap/CHANGELOG.md`.
- `no_alloc = false` servers compile and work (API-21), the first piece of the ww hub (SRV-13): servers generated
  for `std` take and give owned types (`String`, `Vec<T>`, `FooOwned`) and send the same bytes as a `no_alloc`
  server, through shrink_wrap's new `BufWriter::ser_owned` / `write_owned` and `OwnedSer`; `ww api scaffold --alloc`
  matches. Docs page `transport/hub.md`.
- Host event loop: `disconnect().keep_streams()` no longer ends kept streams and sinks when the device closes the medium right after the `Disconnect` (rx took the transport error before tx's word to keep them). See `wire_weaver_client/CHANGELOG.md`.
- Observe (API-39): `ObserveState::reset()` drops the samples taken before it, which went to the next host observing in `Samples` mode with their old times. See the `wire_weaver_core` changelog.
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
- `exact_units` pinned by `rev` in the root `[workspace.dependencies]` (e89f64e, the commit `Cargo.lock` already had): a repo that takes wire_weaver by path (usb_io_fw, vb135_fdcan_iso_usb_fw, typec_dongle) resolved `exact_units` HEAD with its own lock file and broke on `ww_si` re-exports.

## [0.5.0] - 2026-10-15

Workspace version 0.5.0 since 30 Sep 2026, tagged 15 Oct 2026, not on crates.io yet. See the `## Unreleased` sections of
the per-crate changelogs for details and migration notes. Highlights since 0.4.0:

### Added

- Who connects (CLT-41): clients tell the device their program, user, host, pid and kind in link setup (`ww_link::ClientMeta`, wire-compatible appended field), with defaults found by the library and `ClientConfig::client_meta` to override; devices read it with `DeviceLink::peer_meta()`. See the per-crate changelogs of `ww_link`, `ww_device`, `wire_weaver_client` and `wire_weaver_py`.

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
