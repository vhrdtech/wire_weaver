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

## Commands

```sh
just check          # cargo check for repo-root workspace + wire_weaver_usb_link with device/host/defmt features
just check-mcu       # cargo check the separate `mcu/` workspace (embassy-based, embedded)
just check-examples-mcu   # cargo check every board in examples_mcu/ (excluded from root workspace)
just test            # cargo nextest run --workspace --no-fail-fast
just serve-docs       # local docs preview (uv run zensical serve)
just build-docs        # build docs site
just pre-commit         # cargo sort -w && cargo clippy
```

Single test: `cargo nextest run -p <crate> <test_name>` (nextest is required — see `.config/nextest.toml` for the
slow-test timeout). To inspect the code a macro invocation actually generates, uncomment/add
`debug_to_file = "../../target/some_name.rs"` to the `ww_codegen!`/`ww_impl!` call and check that file — this is the
normal way to debug codegen, don't try to reason about macro output blind.

`mcu/`, `examples_mcu/*`, `wire_weaver_tool`, and `tests/usb_link` are **excluded** from the root Cargo workspace
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
  defines `WireWeaverAsyncApiBackend`/`MessageSink` (the sans-IO server-side traits) and `RpcResult`/`GetResult`/
  `SetResult` (method/property call outcomes, including deferred replies). This is what a user's API/firmware/driver
  crate depends on. Generated server code is IO-free by design — transport is always separate.
- **`wire_weaver_client/`** — generated `std` client runtime: event loop, `Commander`, USB/RTT/tracing client glue,
  used by code the `client = "..."` codegen argument produces. No-std client generation doesn't exist yet.
- **Transport crates** — `ww_link` (link-layer abstraction), `ww_framer` (packs many small messages into one
  packet, or splits a big message across several — used by both USB and UDP transports), `wire_weaver_usb_link`
  (USB packet framing, `device`/`host`/`defmt` features), `wire_weaver_udp_link`, `mcu/wire_weaver_usb_embassy`
  (embassy-based USB driver + event loop for the device side, lives in the separate `mcu` workspace),
  `wire_weaver_net_host` (host-side networking).
- **`wire_weaver_cli/`** (binary name `ww`, run via `cargo ww` alias from `.cargo/config.toml`) — CLI with
  introspection and USB loopback subcommands (`src/cmd/`).
- **`wire_weaver_tool/`** — egui-based GUI (Trunk-buildable) for viewing parsed AST / generated code side by side,
  no_std vs host toggle; see `docs/dev_tool.md` for current/planned features.
- **`ww_stdlib/`** (separate crates, some vendored here under `ww_stdlib/*`, canonical home is the
  `vhrdtech/ww_stdlib` repo) — reusable API traits and data types meant to be shared across unrelated projects by
  publishing to crates.io (date/time, version, numeric/SI, GPIO, I2C, SPI, UART, CAN bus, DFU, logging,
  introspection via `ww_self`). Prefer reusing/extending one of these over inventing a project-local equivalent.
- **`examples/`** — paired `<name>_api` (trait + types, no_std-compatible) / `<name>` (server+client wiring, tests)
  crates; this pairing is the intended project shape end users should copy (see `docs/api/folder_structure.md`).
  `examples_mcu/` has real firmware targets per dev board (excluded from root workspace).
  `examples/compare_wire_formats` benchmarks `shrink_wrap` against other formats.
- **`tests/`** — one integration-test crate per API feature (`methods`, `properties`, `streams`,
  `array_of_streams`, `traits`), each with a `<name>_api` companion crate defining the trait/types under test —
  this is the best place to see minimal working examples of a specific macro argument or feature combination.
  `tests/usb_link` is excluded from the root workspace (its own toolchain needs).
- **`fuzz/`** — fuzzes `ww_framer` tx/rx round-tripping (`cargo fuzz run framer-tx-rx`); `wire_weaver_usb_link` has
  its own nested `fuzz/` too.

## The core pattern to recognize

A trait is declared with `#[ww_trait]` (or `#[ww_api_root]` for the top-level one) — this expands to nothing by
itself, it's a marker. Elsewhere, `wire_weaver::ww_codegen!(my_api_crate :: MyTrait for MyStruct, server = true, client = "...", no_alloc = ..., use_async = ..., method_model = "...", property_model = "...")`
re-parses that trait's source and generates the actual serdes + dispatch (server) or call-builder (client) code
implemented on `MyStruct`. Data types crossing the wire use `#[derive_shrink_wrap(...)]`, not plain `#[derive]`
(see `docs/serdes/derive.md` for the full directive reference — `borrowed`/`owned`, `final_structure` /
`self_describing` / `sized`, `ww_repr`, `#[default = ..]` for evolvable fields, etc.). Grep `tests/*/src/lib.rs` for
worked examples of any specific combination before writing new codegen-invoking code from scratch.

## Compatibility rules (don't guess — check `docs/evolution/rules.md`)

Types default to `Unsized` (fully evolvable: fields can be appended with `#[default = ..]`, renamed but not
reordered). `final_structure`, `self_describing`, and `sized` trade evolvability for a smaller wire size and have
different, more restrictive rules once chosen — they cannot be un-chosen later. An API's data types are part of its
SemVer contract: breaking a type used in the API requires bumping the API crate's major version, and the API model
crate itself (e.g. `ww_client_server`) participates in that same compatibility surface (see `docs/api/folder_structure.md`
for why the API crate's own name+version acts as a global identifier). Run the evolution checker
(`docs/evolution/checker_tool.md`) rather than eyeballing compatibility when in doubt.

Gotcha: for a `sized` enum, the derive macro's compile-time assertion only checks that `ELEMENT_SIZE` is the
`Sized { .. }` *variant*, not that `size_bits` is numerically correct — and for non-`unib32` reprs, the
discriminant's own bits are never folded into that constant (only payload field sizes are summed; see
`shrink_wrap_derive/src/codegen/item_enum.rs`). A fieldless `ww_repr = u2, sized` enum can report
`ELEMENT_SIZE = Sized { size_bits: 0 }` even though it actually writes 2 bits on the wire. Actual serialization is
still byte-correct (it's driven by `BufWriter`'s live bit cursor, not by this constant), but don't trust
`size_bits` for manual space math — verify with a real `to_ww_bytes()` call instead (see "Editing `docs/`" below).

## Editing `docs/`

`just build-docs` runs zensical's own link/anchor checker across the whole site, not just a build — it flags a
`page does not exist` or `anchor does not exist` warning for any broken internal link, including cross-file ones
(`other.md#some-heading`). Run it after touching any `docs/*.md` and treat new warnings it reports as bugs to fix
(some pre-existing ones predate this instruction, e.g. `docs/serdes/derive.md`'s self-link to `ww_repr`). Anchor
slugs are generated by lower-casing the heading and collapsing every run of non-alphanumeric characters (spaces,
backticks, `=`, `<>`, `:`, ...) to a **single** hyphen — a heading like `` ### `ww_repr = <repr>` (enums only) ``
becomes `#ww_repr-repr-enums-only`, not `#ww_repr--repr-enums-only`; don't guess, grep the built `site/**/index.html`
for `id="..."` if unsure. Adding a new page also requires a manual entry in `zensical.toml`'s `nav` list — it's not
inferred from the file tree. `site/` is build output, not checked in; `rm -rf` it when done poking at it.

When a docs page makes a specific claim about serialized bytes or bit layout (as `docs/serdes/showcase.md` does
throughout), don't hand-derive it — add a temporary `examples/scratch_*.rs` in the relevant crate (usually
`shrink_wrap/shrink_wrap/`), run it with `cargo run --example scratch_*`, copy the real output into the docs, then
delete the example. `derive.md`'s own examples follow this same "checked against the current macro implementation"
rule.

## Naming convention

`wire_weaver_` prefix = core crates implementing the framework itself. `ww_` prefix = crates built on top of
WireWeaver providing reusable types/traits/tools (`ww_stdlib` and friends) — use this prefix too for anything
you write that's meant to be reusable across projects, not project-specific.
