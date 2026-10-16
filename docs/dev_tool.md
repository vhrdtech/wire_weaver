---
level: advanced
---

# Dev GUI tool

`wire_weaver_tool` is an [egui](https://github.com/emilk/egui) app (native, or in the browser through Trunk) for
looking at what WireWeaver makes of an API definition: the parsed AST and the generated code side by side.

!!! warning "Out of date"
    The tool was last updated in June 2025 and does not build against the current `wire_weaver_core` (it uses
    modules that have since been reorganized, such as `wire_weaver_core::ast`). It lives outside the repository's
    Cargo workspace, so CI doesn't build it either. Status and plans: TOOL-6 in the
    [feature tracker](features.md).

For the same jobs today:

- `ww api tree <path>` and `ww api ast <path>` print an API crate's resource tree and parsed bundle, see
  [`ww api`](cli_reference.md#ww-api).
- `debug_to_file = "../../target/some_name.rs"` in a `ww_codegen!` / `ww_impl!` call writes the generated code to a
  file.
- `ww introspect` and the [Python module](python.md) show and call a connected device's API.

## What it did

- Load and parse a source file with type and API definitions, reload or reparse it
- Show the internal AST and parser warnings and errors
- Show the generated serdes code of each struct, enum and const
- Show the generated server dispatcher and client code
- A `no_alloc` switch to compare embedded and host code

## Planned

- Input and output widgets for the types (numbers with SI units as spinner, dial or slider, strings, colors, ...)
- Documentation-like UI that can call a live server
- Server mockup UI answering with user input, recorded answers or examples
- Loading types and API from bytecode, and compiling sources to bytecode (through Rust FFI or a backend service)
