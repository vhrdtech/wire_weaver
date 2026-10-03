# AGENTS.md

This file provides guidance to all AI code assistants when working with code in this repository.

## What this is

WireWeaver is a collection of Rust crates for designing `#[no_std]` device APIs (RPC methods, streams, properties) with a
custom wire format called `shrink_wrap`: zero-copy, no-alloc, bit/nibble-packed, and built-in backwards/forwards
compatibility. It generates matching server (device/firmware) and client (host) code from a single trait
definition, for both `no_std` and `std`, sync/async/blocking/promise flavors.

While primary focus is no_std and no-alloc, std use have it's merits as well - wire format is very dense,
which can be handy when storing large amount of small objects without resorting to compression.

Read `docs/index.md` first, then drill into `docs/serdes/shrink_wrap.md` (wire format), `docs/serdes/derive.md`
(the `#[derive_shrink_wrap]` macro — very detailed, read before touching any type definitions), `docs/serdes/showcase.md`
(worked, byte-verified tricks — bit packing, `UNib32`, `Option`/`Result` as flags, `RefBox` self-reference, the
patch-the-discriminant-later builder pattern), `docs/api/overview.md` (methods/streams/properties/traits), and
`docs/evolution/rules.md` (what changes are wire-compatible). The docs site source (mkdocs/zensical) lives under
`docs/`; prefer it before digging into source, but if unclear, source and comments in it are authoritative.

`FEATURES.md` is the feature tracker and roadmap (done, in progress, planned, per area and target release). Check it
before starting a feature, and update the matching item in the same change when finishing one (see the conventions
at its top). It is also the docs site's "Features" page, pulled in by `docs/features.md`, so edit the root file only.

## Commands

```sh
just check          # cargo check for repo-root workspace (+ ww_device with defmt/embassy-time), mcu/ and examples_mcu/
just check-mcu       # cargo check the separate `mcu/` workspace (embassy-based, embedded)
just check-examples-mcu   # cargo check every board in examples_mcu/ (excluded from root workspace)
just test            # cargo nextest run --workspace --no-fail-fast
just serve-docs       # local docs preview (uv run zensical serve)
just build-docs        # build docs site
just pre-commit         # cargo sort -w -g && cargo clippy
just save-snapshots       # save API snapshots of ww_global and ww_stdlib/*, copy them into wire_weaver_snapshots
```

Single test: `cargo nextest run -p <crate> <test_name>` (nextest is required — see `.config/nextest.toml` for the
slow-test timeout). To inspect the code a macro invocation actually generates, uncomment/add
`debug_to_file = "../../target/some_name.rs"` to the `ww_codegen!`/`ww_impl!` call and check that file — this is the
normal way to debug codegen, don't try to reason about macro output blind.

CI (`.github/workflows/ci.yml`) fails on any warning: `cargo fmt --check`, `cargo sort -w -g --check` (plain
`cargo sort` flattens the grouped `[workspace.dependencies]`, always pass `-g`), `typos` (project words go into
`typos.toml`), clippy and rustdoc with `-D warnings` (also `--all-features`), nextest + doctests (default features
only, `defmt` breaks linking host tests), `cargo semver-checks` of crates already on crates.io, and `cargo check` of
`mcu/` and every `examples_mcu/` board. Run the matching command locally before declaring a change done. Crates
under `examples/` and `tests/` are `publish = false`, keep it that way for new ones.

`mcu/`, `examples_mcu/*`, and `wire_weaver_tool` are **excluded** from the root Cargo workspace
(different targets/toolchains — embedded, egui GUI) and must be built from their own directory or via the `just`
recipes above.

## Workspace layout and crate roles

The codegen pipeline (read `wire_weaver_derive` → `wire_weaver_core` in that order to understand macro expansion):

- **`shrink_wrap/`** — the wire format itself, no dependency on the rest of the workspace. `shrink_wrap/shrink_wrap`
  has `BufReader`/`BufWriter` and the core traits (`SerializeShrinkWrap`/`DeserializeShrinkWrap` + `..Owned`
  variants); `shrink_wrap/shrink_wrap_derive` implements the `#[derive_shrink_wrap(..)]` proc macro. Usable
  completely standalone if you just need a serialization format.
- **`wire_weaver_derive/`** — thin proc-macro crate exposing `ww_trait`/`ww_api_root` (attribute, marks a trait
  definition — produces no code, just a marker parsed back out of source), `ww_codegen!`/`ww_impl!` (the macros that
  actually re-parse the marked trait's source file and emit server/client code), and `full_version!`/`compact_version!`.
- **`wire_weaver_core/`** — where the real work happens: `transform/` turns a `ww_trait`-annotated source file into
  an AST (`api.rs`, `ty.rs`, `crate_walker.rs` — it literally walks and re-parses dependency crates' source to find
  trait defs), `codegen/` turns that AST into server code (`codegen/server`), client code (`codegen/client`), and
  shared type/serdes code (`ty_def.rs`). `method_model`/`property_model` (with `.pest` grammars in
  `wire_weaver_core/grammar/`) parse the small `"pattern=keyword, ..."` DSL used in `method_model = "..."` /
  `property_model = "..."` macro arguments. Exposes `gen_client`/`gen_server` for use from a `build.rs` as an
  alternative to the proc macro.
- **`wire_weaver/`** — the user-facing crate: re-exports `shrink_wrap`, the derive/codegen macros, `ww_version`, and
  defines `WireWeaverAsyncApiBackend`/`WireWeaverApiBackend` (the sans-IO server-side traits), the handler
  `Context` (request seq, medium, `EventOut`/`BlockingEventOut` to send stream/property updates and deferred replies,
  `EventWriter`/`EventQueue`) and `RpcResult`/`GetResult`/`SetResult` (method/property call outcomes, including
  deferred replies). This is what a user's API/firmware/driver
  crate depends on. Generated server code is IO-free by design — transport is always separate.
- **`wire_weaver_client/`** — generated `std` client runtime: event loop, `Commander`, USB/RTT/tracing client glue,
  used by code the `client = "..."` codegen argument produces. No-std client generation doesn't exist yet.
- **Transport crates** — `ww_link` (link-layer abstraction), `ww_framer` (packs many small messages into one
  packet, or splits a big message across several — used by both USB and UDP transports), `ww_device` (device side:
  sans-IO `DeviceLink` + async `Server` with `wait()`/`handle()` for a user-owned event loop + `blocking::Server`,
  RTT and WebSocket media behind the `rtt` / `ws` features, `embassy-net` sockets with `embassy-net`),
  `mcu/wire_weaver_usb_embassy` (embassy-usb class and packet IO on top of `ww_device`,
  lives in the separate `mcu` workspace). Host-side transports (USB, RTT, WebSocket, UDP, in-process) live in
  `wire_weaver_client` behind features, each a `Transport` impl (`event_loop/transport.rs`) driven by the shared
  event loop core.
- **`wire_weaver_cli/`** (package `wire_weaver_cli`, binary name `ww`, run via `cargo ww` alias from `.cargo/config.toml`) — CLI with
  introspection and USB loopback subcommands (`src/cmd/`).
- **`wire_weaver_tool/`** — egui-based GUI (Trunk-buildable) for viewing parsed AST / generated code side by side,
  no_std vs host toggle; see `docs/dev_tool.md` for current/planned features.
- **`ww_stdlib/`** (separate crates, some vendored here under `ww_stdlib/*`, canonical home is the
  `vhrdtech/ww_stdlib` repo) — reusable API traits and data types meant to be shared across unrelated projects by
  publishing to crates.io (date/time, version, numeric/SI, GPIO, I2C, SPI, UART, CAN bus, DFU, logging).
  Prefer reusing/extending one of these over inventing a project-local equivalent.
- **`ww_self/`**, **`ww_global/`** — framework-level base types that live in this repo, not in `ww_stdlib`:
  `ww_self` is the API model AST used for introspection and API snapshots, `ww_global` is the global type ID
  registry. Versioned like API crates (own version, the version is the identity, see `docs/evolution/rules.md`).
- **`wire_weaver_snapshots/`** — API snapshots (`api_snapshots/*.ron`, saved by `ww api save`) of `ww_global` and
  `ww_stdlib/*`, copied in and embedded. Codegen leaves traits and types known from them out of the introspection
  data a device sends, `wire_weaver_client` puts them back. After changing any trait or type in those crates, run
  `just save-snapshots` (a `wire_weaver_core` test fails otherwise); `--force` only for versions never published.
- **`examples/`** — paired `<name>_api` (trait + types, no_std-compatible) / `<name>` (server+client wiring, tests)
  crates; this pairing is the intended project shape end users should copy (see `docs/api/folder_structure.md`).
  `examples_mcu/` has real firmware targets per dev board (excluded from root workspace).
  `examples/compare_wire_formats` benchmarks `shrink_wrap` against other formats.
- **`tests/`** — one integration-test crate per API feature (`methods`, `properties`, `streams`,
  `array_of_streams`, `traits`), each with a `<name>_api` companion crate defining the trait/types under test —
  this is the best place to see minimal working examples of a specific macro argument or feature combination.
  `tests_common::start_device` runs the generated server on `ww_device::Server` in a device thread, connected through
  `wire_weaver_client`'s `in_process` transport, so tests go through the real host event loop (link setup, timeouts,
  streams); `TestDevice::drop_requests` simulates a stuck device, `TestDevice::send` pushes stream events.
  The device side is tested end-to-end against the real host event loop in
  `wire_weaver_client/src/event_loop/device_e2e_tests.rs` (in-memory packets, real framers on both ends).
- **`fuzz/`** — fuzzes `ww_framer` tx/rx round-tripping (`cargo fuzz run framer-tx-rx`).

## The core pattern to recognize

A trait is declared with `#[ww_trait]` (or `#[ww_api_root]` for the top-level one) — this expands to nothing by
itself, it's a marker. Elsewhere, `wire_weaver::ww_codegen!(my_api_crate :: MyTrait for MyStruct, server = true, client = "...", no_alloc = ..., use_async = ..., method_model = "...", property_model = "...")`
re-parses that trait's source and generates the actual serdes + dispatch (server) or call-builder (client) code
implemented on `MyStruct`. Data types crossing the wire use `#[derive_shrink_wrap(...)]`, not plain `#[derive]`
(see `docs/serdes/derive.md` for the full directive reference — `borrowed`/`owned`, `final_structure` /
`self_describing` / `sized`, `ww_repr`, `#[default = ..]` for evolvable fields, etc.). Grep `tests/*/src/lib.rs` for
worked examples of any specific combination before writing new codegen-invoking code from scratch.

## Server handlers: scaffold and sync with the API

The generated server dispatcher calls handlers the user implements on the server struct (`fn <name>`,
`get_`/`set_<name>`, `changed_<name>`, `sideband_`/`write_<name>`, `valid_indices_root_<..>`, prefixed with the trait
item chain for nested traits). Their exact names and signatures depend on the `ww_codegen!` arguments, so don't write
them by hand — `ww api scaffold <api_crate> [--name Trait]` (`cargo ww api scaffold ...`) prints the struct, a stub for
every handler (`Unimplemented.into()`, empty bodies, no valid indices) and the matching `ww_codegen!` call. Pass the
same options the target `ww_codegen!` uses: `--use-async`, `--method-model`, `--property-model`, `--server <Struct>`,
`--alloc` for `no_alloc = false`.

To bring an existing server in line after the API changed (or to fill in a partly written one):

1. Find its `ww_codegen!(api :: Trait for path::Server, server = true, ..)` call and read the options from it.
2. Run `ww api scaffold` with those options into the scratchpad — this is the expected handler list.
3. Collect the existing handlers: every `impl Server` block, in any inline or out-of-line `mod`, across files.
4. Compare by name, then by signature. Treat as equal what compiles the same: type paths vs. imported names
   (`shrink_wrap::RefVec` vs `RefVec`), elided vs `'_` lifetimes, `impl EventOut` vs a generic bound, `&self`
   where `&mut self` is expected, parameter names.
5. Edit in place, don't regenerate the file: add missing stubs next to the handlers of the same trait level, fix
   mismatched signatures keeping the body and the user's parameter names, add missing `value_on_changed` struct
   fields. Don't delete handlers that aren't expected — they may be stale resources or helper methods — list them
   for the user instead.
6. `cargo check` the crate; errors in bodies after a signature change are expected, point them out.

Every handler takes `cx: &mut Context<'_, impl EventOut, Medium>` (`BlockingEventOut` for `use_async = false`)
right after `&mut self`; `Medium` comes from `ww_codegen!(.., medium = "..")` (`--medium` for the scaffold), `()` by
default. Deferred methods answer later with `<method>_send_return(out, cx.reply_to().seq, ..)`.

Known server codegen limitation the scaffold can't work around: `no_alloc = false` servers don't compile.

## Compatibility rules (don't guess — check `docs/evolution/rules.md`)

Types default to `Unsized` (fully evolvable: fields can be appended with `#[default = ..]`, renamed but not
reordered). `final_structure`, `self_describing`, and `sized` trade evolvability for a smaller wire size and have
different, more restrictive rules once chosen — they cannot be un-chosen later. An API's data types are part of its
SemVer contract: breaking a type used in the API requires bumping the API crate's major version, and the API model
crate itself (e.g. `ww_client_server`) participates in that same compatibility surface (see `docs/api/folder_structure.md`
for why the API crate's own name+version acts as a global identifier). Run the evolution checker
(`docs/evolution/checker_tool.md`) rather than eyeballing compatibility when in doubt.

**Before making any breaking change — wire or Rust API, in any crate — read `docs/evolution/rules.md`** and follow
it. It explains the crate layers (`shrink_wrap` → `ww_stdlib` → framework → user API), why a `shrink_wrap` break
cascades to every other crate, which framework code is actually wire-defining (`ww_framer`, `ww_link`,
`ww_client_server`, server/client codegen), and which version position to bump. Tell the user explicitly when a change
breaks the wire, and which deployed devices/hosts it affects.

Gotcha: for a `sized` enum, the derive macro's compile-time assertion only checks that `ELEMENT_SIZE` is the
`Sized { .. }` _variant_, not that `size_bits` is numerically correct — and for non-`unib32` reprs, the
discriminant's own bits are never folded into that constant (only payload field sizes are summed; see
`shrink_wrap_derive/src/codegen/item_enum.rs`). A fieldless `ww_repr = u2, sized` enum can report
`ELEMENT_SIZE = Sized { size_bits: 0 }` even though it actually writes 2 bits on the wire. Actual serialization is
still byte-correct (it's driven by `BufWriter`'s live bit cursor, not by this constant), but don't trust
`size_bits` for manual space math — verify with a real `to_ww_bytes()` call instead (see "Editing `docs/`" below).

## Editing `docs/`

`just build-docs` runs zensical's own link/anchor checker across the whole site, not just a build — it flags a
`page does not exist` or `anchor does not exist` warning for any broken internal link, including cross-file ones
(`other.md#some-heading`). Run it after touching any `docs/*.md` and treat new warnings it reports as bugs to fix
(some pre-existing ones predate this instruction, e.g. `docs/serdes/derive.md`'s self-link to `ww_repr`).
But only after asking user about it, since running `just build-docs` while user had `just serve-docs` breaks that instance.

Anchor slugs are generated by lower-casing the heading and collapsing every run of non-alphanumeric characters (spaces,
backticks, `=`, `<>`, `:`, ...) to a **single** hyphen — a heading like ``### `ww_repr = <repr>` (enums only)``
becomes `#ww_repr-repr-enums-only`, not `#ww_repr--repr-enums-only`; don't guess, grep the built `site/**/index.html`
for `id="..."` if unsure. Adding a new page also requires a manual entry in `zensical.toml`'s `nav` list — it's not
inferred from the file tree. `site/` is build output, not checked in; `rm -rf` it when done poking at it.

When a docs page makes a specific claim about serialized bytes or bit layout (as `docs/serdes/showcase.md` does
throughout), don't hand-derive it — add a temporary `examples/scratch_*.rs` in the relevant crate (usually
`shrink_wrap/shrink_wrap/`), run it with `cargo run --example scratch_*`, copy the real output into the docs, then
delete the example. `derive.md`'s own examples follow this same "checked against the current macro implementation"
rule.

## Changelogs and commits

Every user-visible change (features, fixes, breaking changes) gets an entry under `## Unreleased` in the
`CHANGELOG.md` of each affected crate, written in the same change — don't leave it for later. Follow the existing
format (`### ⚠️ Breaking`, `### 🚀 Features`, `### 🐛 Fixes`), name the public items involved, and for breaking changes
say what users must change. Create the file if a crate doesn't have one yet.

Every change to a crate also bumps its **minor** version (`0.4.0` → `0.5.0`; pre-1.0, minor is the SemVer-breaking
position, so don't try to decide whether a patch bump would do). Exceptions, see `docs/evolution/rules.md`: purely
additive changes to `shrink_wrap`/`shrink_wrap_derive` and wire-compatible additions to API crates (`ww_stdlib/*`,
`*_api`) bump **patch**, so the rest of the ecosystem doesn't have to be re-released and old devices still connect. Bump once per release cycle: if the crate's version
is already above its latest crates.io release (check with `cargo info --registry crates-io <crate>`; plain
`cargo info` inside the repo shows the local version), it has been bumped and stays as is. Never-published crates
are left alone. How to bump:

- Crates with `version.workspace = true` (`wire_weaver`, `wire_weaver_core`, `wire_weaver_derive`,
  `wire_weaver_client`, `wire_weaver_cli`, `ww_device`, `ww_link`, `ww_framer`, ...) share `[workspace.package]
  version` in the root `Cargo.toml` — bump that one, never give them their own version.
- Path dependencies carry a version too (`cargo publish` rejects them otherwise). They are all declared once in the
  root `[workspace.dependencies]`, so the version is updated there; also update version strings in `docs/` (e.g.
  `docs/serdes/derive.md`) and `mcu/Cargo.lock` (run `just check-mcu`).
- For API crates (`ww_stdlib/*`, `*_api`), the version is part of the API's identity (see
  `docs/api/folder_structure.md`), so the bump is exactly what tells old and new APIs apart — don't skip it.

Keep dependencies and metadata in the root workspace: every crate inherits `authors`, `edition`, `license` and
`repository` with `key.workspace = true` (except `ww_stdlib/*`, whose `repository` is `vhrdtech/ww_stdlib`), every in-repo crate and every external crate used by more than one member is
declared in `[workspace.dependencies]` and used as `dep.workspace = true` (plus `features`/`optional` as needed). When
the workspace entry has `default-features = false`, a member that needs the defaults lists them in `features` (usually
`["std"]`); the reverse doesn't work, a member can't turn defaults off if the workspace entry keeps them. The only
inline exceptions are a few crates that must disable defaults of a dependency other members use with defaults
(`semver` in `ww_version`, `either` in `shrink_wrap`). `mcu/`, `examples_mcu/*` and
`wire_weaver_tool` are separate workspaces and keep their own.

Commit messages use Conventional Commits with a scope (`feat(usb): ...`, `fix(client): ...`): a short imperative
summary line, a blank line, then a body explaining what changed and why, with a bullet per crate or area for
multi-crate changes.

Never commit on your own initiative. When a change is done, update the changelogs, then show the proposed commit
message and the list of files to be staged, and ask the user before running `git commit`. Approval covers only that one
commit, not later ones.

## Naming convention

`wire_weaver_` prefix = core crates implementing the framework itself. `ww_` prefix = crates built on top of
WireWeaver providing reusable types/traits/tools (`ww_stdlib` and friends) — use this prefix too for anything
you write that's meant to be reusable across projects, not project-specific.

## Versions

Every commit with real work bumps the version in the same commit (manifest + CHANGELOG entry), so any build
traces back to a commit:
- Patch for fixes and small changes, minor for features or anything breaking before 1.0, major only when the
  owner says so. In a workspace, only the crates that changed.
- Docs-only, CI-only and no-behaviour-change refactors skip it; a burst of follow-up fixes shares one bump.
- CLIs print version, git SHA and build time in `--version`, e.g. `tool 0.4.2 (a1b2c3d-dirty, built 3 Oct 2026
  18:20)`: a small `build.rs` without extra crates (`git rev-parse --short HEAD`, `-dirty` when
  `git status --porcelain` isn't empty, `rerun-if-changed` on `.git/HEAD` and `.git/index`, `unknown` without
  git). Firmware reports the same through `fw_info`. When touching a CLI that lacks it, add it.
