# AGENTS.md

Guidance for AI code assistants working in this repository.

## What this is

WireWeaver is a set of Rust crates for designing `#[no_std]` device APIs (methods, streams, properties) with its own
wire format, `shrink_wrap`: zero-copy, no-alloc, bit/nibble-packed, backwards and forwards compatible. One trait
definition generates the server (firmware) and client (host) code, `no_std` and `std`, sync/async/blocking/promise.
The dense wire format is useful on `std` too (many small objects without compression).

Read `docs/index.md` first, then `docs/serdes/shrink_wrap.md` and `wire_format.md`, `docs/serdes/derive.md` with
`derive_reference.md`, `size_assumptions.md` and `layouts.md` (read before touching any type definition),
`docs/serdes/showcase.md` (byte-verified tricks), `docs/api/overview.md`, `docs/types.md`, and
`docs/evolution/rules.md`. Prefer the docs (`docs/`, built with zensical) to the source; where they are unclear, the
source and its comments are authoritative.

## Numbers are typed: never a bare integer or float

Almost every number in an API type (property, method argument or return, stream item, field of a
`#[derive_shrink_wrap]` type) is typed, not a bare `u32`/`u64`/`f32` with its meaning in the name (`timeout_ms`,
`item_count`, `scale_id`) or in a comment (`/// percent`): a physical quantity is an `exact_units` / `ww_si` type, a
count is a counted unit, an identifier is a newtype, a position or sequence number is `Index`/`IndexFrom1`/`Seq`.
`Fixed` and a single-field newtype are exactly their `T` on the wire, so this costs no byte and no cycle (DER-20),
and clients, `ww`, Python and the evolution checker then know what the number means. A bare integer or float stays
only with a reason: a raw protocol field (a register, a wire-format byte) or a truly dimensionless scale.

| What                              | Type                                                                         |
|-----------------------------------|------------------------------------------------------------------------------|
| durations, timeouts, periods      | `Millis<u16>`, `Micros<u32>`, `Nanos<T>`, `Fixed<Second, One, u32>`          |
| sizes                             | `Bytes<u32>`, `Fixed<Byte, Kibi, u16>`, `Fixed<Bit, One, T>`                 |
| ratios, percent, ppm              | `Fixed<Ratio, Centi, u8, Range<0, 100>>`, `Ppb<i32>`, `Duty16`, `Q15`        |
| any physical value                | `Fixed<Volt, Milli, i16>`, `Fixed<Hertz, Kilo, u32>`, ADC codes as `FullScale` |
| temperatures, headings           | `Point<Celsius, Fixed<Kelvin, Centi, i16>>`, `Point<Angle<Degree>, ..>`     |
| counts                            | `Fixed<Item, One, u32>` (a counted unit; declare your own with `unit!`)      |
| identifiers                       | a newtype: `pub struct ScaleId(u8);` (`#[derive_shrink_wrap]`)              |
| positions in a table, channels    | `Index<Sample, u32>`, `IndexFrom1<Item, u8, Range<1, 4>>`                   |
| sequence numbers                  | `Seq<Frame, u16>` (wraps, RFC 1982 compare)                                  |
| device timestamps                 | `ww_si::Timed<T>`, `Second` from the `ww_timebase` clock                     |
| unit chosen per device at run time | `ww_si::Quantity<U>` (prefix and number type on the wire), `AnyQuantity`    |

- **Pick `T`** as the smallest storage that holds the largest value at the chosen step: integers by default
  (`u16` ms up to 65 s, `u32` µs up to 71 min, `u64` µs for timestamps), signed only when the value can be negative,
  `f32`/`f64` only for values without a fixed resolution. The step (`P`: `Milli`, `Centi`, `Kibi`, `Pow10<E>`,
  `Rational<N, D>`, `FullScale<V, BITS>`) is the device's real resolution.
- **Bounds** (`B`, default `Unbounded`): `Range<MIN, MAX>` (inclusive, `RangeU64`, `RangeDec` for floats) whenever
  the device can't take every value of `T`. They are checked on decode, so an out-of-range `set` is refused before
  the handler runs and clients refuse it before sending.
- **Counted things** (`Sample`, `Frame`, `Packet`, `Item`, ...) are units. An API declares its own unit with
  `unit!`, step with `scale!` and a register formula's zero with `origin!` (`tests/quantities_api/src/lib.rs`).
- **Not yet**: instants with a named epoch (Unix, GPS; STD-65 instants-named-epochs) don't exist, so a wall-clock
  timestamp is `Micros<u64>` with the epoch in its doc comment; sentinel codes inside a value (STD-66
  sentinels-next-to-bounds) don't either, use `Option<Fixed<..>>`; the C consumer reads a `Fixed` as its `T` but has
  gaps for `Point`/`Index`/`Seq` (API-26); `serde` forms are STD-78.
- `ww api check` and `ww api scaffold` print a hint for a plain integer whose name ends in a unit suffix (TOOL-22);
  silencing it with `#[no_unit_hint]` needs a reason (a register value, a raw protocol field).

```rust
// Before: the unit lives in names and comments, nothing checks it
pub struct Status { pub uptime_s: u32, /** percent */ pub battery: u8, /** °C x100 */ pub temp: i16,
                    pub frame_seq: u16, pub timeout_ms: u16 }
// After: the same bytes, the unit and bounds in the type
use ww_si::{Fixed, Millis, Point, Range, Seq, origins::Celsius, prefix::{Centi, One},
            units::{Frame, Kelvin, Ratio, Second}};
pub struct Status {
    pub uptime: Fixed<Second, One, u32>,
    pub battery: Fixed<Ratio, Centi, u8, Range<0, 100>>,
    pub temp: Point<Celsius, Fixed<Kelvin, Centi, i16>>,
    pub frame: Seq<Frame, u16>,
    pub timeout: Millis<u16>,
}
```

A count: `item_count: u32` becomes `items: Fixed<Item, One, u32>`. An id: `scale_id: u8` becomes `scale: ScaleId`
(`#[derive_shrink_wrap] pub struct ScaleId(u8);` defined once, reused everywhere that id crosses the wire).

Docs: `docs/std_library/exact_units.md` (`Fixed`, scales, bounds, aliases, in clients), `ww_si.md` (`Quantity`,
units, dimensions, timestamps), `points.md`, `indices.md`, `rounding.md`. `exact_units` lives in its own repository
(romixlab/exact_units), taken by git and locked by commit in `Cargo.toml`; `ww_si` re-exports all of it, so an API
writes `ww_si::..` and turns on the unit groups it needs as `ww_si` features (`time`, `electrical`, `counting`, ...).

## FEATURES.md

`FEATURES.md` is the feature tracker and roadmap, the only place status is kept (no README checklists, TODO files or
external trackers). Check it before starting a feature and update the matching item in the same change when finishing
one (conventions at its top). It is also the docs' "Feature tracker" page: edit the root file only, and an item that
becomes done or in progress needs a line in `tests/features_page/entries.md`; then run `just features-page` (a test
checks both).

- IDs are stable per area (`SW-3`, `SRV-4`, `CLT-12`): never renumber or reuse one, new items take the next free
  number of their area. Use them in commit messages, CHANGELOG entries and `// TODO(SRV-4): ...`.
- When talking to the user, name an ID with a 2-4 word kebab-case slug from its title: `CLT-12
  stream-receive-timeout`, never bare. Commits, CHANGELOG and TODOs keep the bare ID.

## Commands

```sh
just check            # cargo check: root workspace (+ ww_device/ww_si feature sets), mcu/, every examples_mcu/ board
just test --changed   # nextest of the crates changed against main and their dependents; before tpm work ready
just test             # everything (test-all: nextest, feature-set runs, Python conformance); the tpm land gate runs it
just lint             # what CI checks besides tests: fmt, cargo sort -w -g, typos, clippy and rustdoc -D warnings, C
just install          # build ww (--profile deploy) and install it (tpm install, or cargo install without tpm)
just build-docs       # build the docs site and check its links
just save-snapshots   # API snapshots of ww_global, ww_tap, ww_stdlib/* and exact_units into wire_weaver_snapshots
just conformance-update   # fill in golden bytes of new conformance cases
```

`just --list` has the rest (fuzzing, QEMU, hardware, Python). Single test: `cargo nextest run -p <crate> <test>`
(nextest is required). To see what a `ww_codegen!`/`ww_impl!` call generates, add `debug_to_file =
"../../target/some_name.rs"` to it and read that file; don't reason about macro output blind.

CI (`.github/workflows/ci.yml`) fails on any warning; run `just lint` and the matching tests before calling a change
done. Plain `cargo sort` flattens the grouped `[workspace.dependencies]`, always pass `-g`; project words go into
`typos.toml`. Tests run with default features only (`defmt` breaks host linking). Crates under `examples/` and
`tests/` are `publish = false`. `mcu/`, `examples_mcu/*` and `wire_weaver_tool` are separate workspaces (embedded
targets, egui): build them from their directory or through `just`.

### Test time limits

`.config/nextest.toml` kills a test after 3 s on purpose, so a slow test fails and gets looked at. A test near 3 s
(alone or under parallel load) is a bug to find first. Only tests heavy by design (fuzz-like loops, power-cut runs,
re-parsing every snapshot crate) get a per-test override with a comment saying why and how long they take; never
raise the default. Tests don't assert wall-clock rates (`tests/observe` drives the device with a fake clock). Check
timing changes with `cargo nextest run --stress-count N` under CPU load.

### Conformance matrix

The wire format has several implementations on purpose (the derive's layout, and the `ww_self`-based dynamic client,
Python and C), kept byte-identical by one corpus, `tests/conformance/corpus.json` (TEST-23,
`docs/serdes/conformance.md`). A new or changed wire feature lands only with its cases in the corpus, green across
the consumers; a consumer that can't express a case gets a gap mark naming a `FEATURES.md` item, never a silent
skip. New case: `"bytes": "?"` plus the Rust value in `tests/conformance/src/derive.rs`, then `just
conformance-update`. Golden bytes don't change once in: `frozen_bytes` (TEST-24) fails when a case sealed at the merge
base (or `CONFORMANCE_BASE`) changed or disappeared; the only escape is a new `"wire_change": "EVO-n / reason"` on
the case with that `EVO-n` item and a breaking CHANGELOG entry. `coverage` (TEST-25) fails when a wire-relevant
`ww_self::Type` variant or derive directive has no case and no gap in `tests/conformance/src/coverage.rs`.

## Proper fixes, never a lost capability

- A bug or limit found on the way (codegen, wire format, a test) is fixed where it lives, as its own commit or piece.
  Never sidestep it with renames, special cases or workarounds in the code that found it.
- A removal, replacement or refactor keeps everything that worked before: build the replacement first. Never narrow
  tests, docs or `FEATURES.md` to fit a loss; scope limits what a change adds, not what it may break.
- When such a prerequisite is too big for the change, stop and say so with an estimate and a plan, before taking a
  shortcut.
- Flaky and slow tests: find the cause, never raise a limit.

## Firmware: stack, cycles and flash

The device side runs on small MCUs, often in interrupts: every change to server codegen, `ww_stream`, `shrink_wrap`'s
writers or `ww_stdlib` types firmware uses is checked for stack, cycles and flash, not only correctness.

- **Stack.** Big state (stores, outboxes, `ObserveState`, buffers) lives in a `static`, set up in place: a
  `const fn new` or an `init` into `&'static mut MaybeUninit<_>` (`ObserveState::init`). Never build it by value and
  move it (that once cost a 31-35 KB frame). Check frames (`-Z emit-stack-sizes`, `cargo call-stack`, or QEMU's M3
  with 64 KB RAM); keep the hot path small (an observe cells row is ~50 B).
- **Optimization level.** Measure at the profile firmware ships with and say which: `opt-level = "z"` was 2.4x
  slower than `3` on the Series writers. Generic code is instantiated in the firmware crate, so a per-package
  `opt-level` doesn't help; small hot per-item functions get `#[inline(always)]`.
- **Measure, don't guess.** Instruction counts with `examples_mcu/qemu_icount` (bounded `--max-insns`, `--timeout`,
  trace from QEMU's stderr), cycles with DWT `CYCCNT` on real silicon (the STM32H725 on omarchy-m1), `just
  units-budget` / `units-elf` for unit types. Instructions are not cycles: report both as what they are, with the
  clock. Benchmark bins exit QEMU on a HardFault.
- **Flash.** A choice fixed at build time is a type or const parameter, so the linker drops what isn't used
  (`DeltaPacked` as a type: 15 KB instead of 91 KB). Shared crates don't turn on wide types or optional features for
  everyone (a `u64` in `ww_client_server` once cost every firmware 7.7 KB); hosts get them through `std` / `host`.
- **Data layout.** Per-item generic walks are slow on MCUs: loops over struct-of-arrays columns were 2-4x faster with
  the same bytes. Interrupt-side capture is a few stores and a counter (observe cells), encoding in task context.

## Workspace layout

The codegen pipeline (read `wire_weaver_derive`, then `wire_weaver_core`):

- **`shrink_wrap/`**: the wire format, no dependency on the rest. `shrink_wrap` has `BufReader`/`BufWriter` and the
  `SerializeShrinkWrap`/`DeserializeShrinkWrap` (+ `..Owned`) traits, `shrink_wrap_derive` the `#[derive_shrink_wrap]`
  macro, `shrink_wrap/c` the C runtime. Usable standalone.
- **`wire_weaver_derive/`**: `ww_trait`/`ww_api_root` (markers, expand to nothing), `ww_codegen!`/`ww_impl!` (re-parse
  the marked trait's source and emit server/client code), `full_version!`/`compact_version!`.
- **`wire_weaver_core/`**: `transform/` turns the source into an AST (`api.rs`, `ty.rs`, `crate_walker.rs` walks
  and re-parses dependency crates, `unit_hint.rs`), `codegen/` emits server (`codegen/server`, with the scaffold),
  client (`codegen/client`) and type code (`ty_def.rs`). `method_model`/`property_model` parse the `"pattern=keyword"`
  DSL (grammars in `grammar/`). `gen_client`/`gen_server` serve `build.rs` use.
- **`wire_weaver/`**: the user-facing crate: re-exports, `WireWeaverAsyncApiBackend`/`WireWeaverApiBackend`
  (sans-IO server traits), the handler `Context` (seq, medium, `EventOut`/`BlockingEventOut`), `RpcResult`/
  `GetResult`/`SetResult`. Generated server code is IO-free; transport is separate.
- **`wire_weaver_client/`**: the `std` client runtime (event loop, `Commander`, the dynamic client, `quantity`);
  host transports (USB, RTT, WebSocket, UDP, local socket, in-process, hub) are `Transport` impls behind features
  (`event_loop/transport.rs`). There is no `no_std` client yet (CLT-14).
- **Transport and device**: `ww_link` (link layer), `ww_framer` (packs small messages into packets and splits big
  ones), `ww_device` (sans-IO `DeviceLink`, async `Server` with `wait()`/`handle()`, `blocking::Server`; RTT,
  WebSocket, UDP and `embassy-net` behind features), `mcu/wire_weaver_usb_embassy` (embassy-usb class).
- **Device-side crates**: `ww_stream` (streams, observe), `ww_settings`, `ww_bus_slave` (I2C/SPI slave, SRV-26),
  `mcu/ww_ecc_flash` (`EccSafeFlash`, `docs/api/ecc_flash.md`), `ww_lz4`, `ww_tape`, `ww_defmt_logger`.
- **Host tools**: `wire_weaver_cli` (binary `ww`, `cargo ww`; plugins `ww NAME` run `ww-NAME` from PATH),
  `ww_hub`/`ww_hub_api` (the per-user hub), `ww_clock_sync`, `ww_tap`, `wire_weaver_py` (Python),
  `wire_weaver_evolution` (the compatibility checker), `wire_weaver_tool` (egui viewer, `docs/dev_tool.md`). Nothing
  here depends on another repository by path (`just check-clean-clone`); `exact_units` comes by git.
- **`ww_stdlib/`**: reusable API traits and types for unrelated projects (units, time, numeric, GPIO, I2C, SPI, UART,
  CAN, DFU, logs, ...; canonical home `vhrdtech/ww_stdlib`). Reuse or extend one of these before inventing a
  project-local equivalent.
- **`ww_self/`**, **`ww_global/`**: the API model AST (introspection, snapshots) and the global type ID registry;
  versioned like API crates (`docs/evolution/crate_layers.md`).
- **`wire_weaver_snapshots/`**: API snapshots of the crates `just save-snapshots` covers; codegen leaves types known
  from them out of a device's introspection data and the client puts them back. After changing a type there, run
  `just save-snapshots` (a test fails otherwise). It refreshes the crate's current version, right only for a version
  never published (`cargo info --registry crates-io <crate>`): bump a published one first.
- **`examples/`**: paired `<name>_api` (trait and types, `no_std`) and `<name>` (server and client) crates, the
  project shape users copy (`docs/api/folder_structure.md`); `examples_mcu/` has firmware per board.
- **`tests/`**: one integration-test crate per feature, each with a `<name>_api` companion: the best place for minimal
  working examples of a macro argument or type (`tests/quantities_api` for units). `tests_common::start_device`
  runs the server on `ww_device::Server` in a thread through the real host event loop (`drop_requests`, `send`).
  `tests/conformance` is the conformance matrix.
- **`fuzz/`**: framer and `shrink_wrap` fuzz targets (`just fuzz-*`).

## The core pattern

A trait is declared with `#[ww_trait]` (`#[ww_api_root]` for the top one), a marker. Elsewhere `ww_codegen!(my_api ::
MyTrait for MyStruct, server = true, client = "...", no_alloc = .., use_async = .., method_model = "..",
property_model = "..")` re-parses its source and generates serdes and dispatch (server) or call builders (client).
Types crossing the wire use `#[derive_shrink_wrap(..)]` (directives in `docs/serdes/derive_reference.md`). Grep
`tests/*_api/src/lib.rs` for a worked example of any combination before writing codegen-invoking code.

## Server handlers: scaffold and sync with the API

Handler names and signatures depend on the `ww_codegen!` arguments, so don't write them by hand: `ww api scaffold
<api_crate> [--name Trait]` prints the struct, a stub per handler and the matching `ww_codegen!` call. Pass the same
options as the target `ww_codegen!` (`ww api scaffold --help`: `--use-async`, `--method-model`, `--medium`,
`--alloc`, ...).

To bring an existing server in line after the API changed:

1. Read the options from its `ww_codegen!(api :: Trait for path::Server, server = true, ..)` call.
2. Run `ww api scaffold` with them into the scratchpad: the expected handler list.
3. Collect the existing handlers from every `impl Server` block, in any module or file.
4. Compare by name, then signature; equal is what compiles the same (paths vs imports, elided vs `'_` lifetimes,
   `impl EventOut` vs a generic bound, parameter names).
5. Edit in place: add missing stubs next to their trait level, fix signatures keeping bodies and parameter names, add
   missing `value_on_changed` fields. Don't delete unexpected handlers; list them for the user.
6. `cargo check` the crate and point out body errors from changed signatures.

Every handler takes `cx: &mut Context<'_, impl EventOut, Medium>` (`BlockingEventOut` for `use_async = false`)
after `&mut self`; deferred methods answer later with `<method>_send_return(out, cx.reply_to().seq, ..)`.

## Compatibility

Read `docs/evolution/rules.md` and `docs/evolution/crate_layers.md` before any breaking change, wire or Rust API, in
any crate: they explain the crate layers (`shrink_wrap` → `ww_stdlib` → framework → user API), why a `shrink_wrap`
break cascades, which framework code is wire-defining (`ww_framer`, `ww_link`, `ww_client_server`, codegen), and
which version position to bump. Types default to unsized and evolvable (fields appended with `#[default = ..]`);
`final_structure`, `self_describing` and `sized` trade that for size and can't be undone. A type used in an API is
part of its SemVer contract. Use `ww api check` / `ww api diff` rather than eyeballing. Tell the user when a change
breaks the wire and which deployed devices and hosts it affects.

`ElementSize::Sized { size_bits }` is the bits of the value's own fields, padding excluded (`struct { a: bool, b: u8 }`
is 9 bits in 2 bytes): for byte counts, run a real `to_ww_bytes()`.

## Editing `docs/`

- `just build-docs` runs zensical's link and anchor checker over the whole site; new warnings are bugs to fix. It
  writes `site/` and wipes the cache in the checkout it runs in, breaking a `just serve-docs` there (`pgrep -af
  "zensical serve"`); a session's own worktree is fine. `rm -rf site/` when done.
- Anchors: the heading lower-cased, every run of non-alphanumerics collapsed to one `-`
  (``### `ww_repr = <repr>` (enums only)`` is `#ww_repr-repr-enums-only`); grep the built `site/` if unsure.
- A new page needs an entry in `zensical.toml`'s `nav` and a reading level in its front matter (`level: basic`,
  `advanced` or `nomicon`, `docs/levels.md`).
- Claims about bytes or bit layout are never hand-derived: run a temporary `examples/scratch_*.rs`
  (`cargo run --example scratch_*`), copy the output, delete the example.

## Changelogs, commits and versions

- Every user-visible change gets an entry under `## Unreleased` in the `CHANGELOG.md` of each affected crate, in the
  same change (`### ⚠️ Breaking`, `### 🚀 Features`, `### 🐛 Fixes`; name the public items; for breaking changes say
  what users must change). Create the file if a crate has none. The root `CHANGELOG.md` is the project's history: one
  summary line per change under `[Unreleased]` pointing at the crate changelogs, a summary section per release; don't
  run `git cliff -o` over it. "What's new" is answered from these changelogs, with FEATURES.md for status.
- Commits use Conventional Commits with a scope (`feat(usb): ...`): a short imperative summary, a blank line, a body
  saying what and why, a bullet per crate or area. Work happens on a session branch (`tpm work new SLUG`), one commit
  per finished step with its changelogs. `tpm land` puts it on main when the user agrees; publishes, OTA and prod
  wait for the user.
- Session commits don't bump versions. At landing, a changed crate gets its **minor** bumped (pre-1.0 the breaking
  position), except purely additive `shrink_wrap`/`shrink_wrap_derive` changes and wire-compatible additions to API
  crates (`ww_stdlib/*`, `*_api`), which bump patch; once per release cycle (skip it when the version is already above
  `cargo info --registry crates-io <crate>`); never-published crates and docs/CI-only changes stay. Crates with
  `version.workspace = true` share the root `[workspace.package]` version; path dependencies carry the version in the
  root `[workspace.dependencies]`, also in `docs/` strings and `mcu/Cargo.lock` (`just check-mcu`). For API crates
  the version is the API's identity (`docs/api/folder_structure.md`): never skip it.
- Dependencies and metadata live in the root workspace: crates inherit `authors`, `edition`, `license`, `repository`
  (`ww_stdlib/*` point at `vhrdtech/ww_stdlib`); every in-repo crate and every external crate used by more than one
  member is in `[workspace.dependencies]` and used as `dep.workspace = true`. A workspace entry with
  `default-features = false` makes members list the defaults they need (usually `["std"]`); the reverse doesn't work.
  Inline exceptions: `semver` in `ww_version`, `either` in `shrink_wrap`.
- CLIs print version, git SHA and build time in `--version` (`tool 0.4.2 (a1b2c3d-dirty, built 3 Oct 2026 18:20)`)
  from a small `build.rs` without extra crates; firmware reports the same through `fw_info`.

## Naming

`wire_weaver_` crates implement the framework itself; `ww_` crates are built on it as reusable types, traits and
tools: use `ww_` for anything meant to be reused across projects.
