## Unreleased

### ⚠️ Breaking

- `#[ww_trait(N)]` / `#[ww_api_root(N)]` with a global id are a compile error (API-20): ids are not written in code, they live in `ww_global/wire_weaver_gid.json` keyed `crate::Trait`. `<TRAIT>_COMPACT_GID` is looked up there by crate and trait name, so every `ww_stdlib` trait has one now. `compact_version!(Trait)` takes the trait name (`compact_version!()` the crate) instead of an id expression.
- `#[ww_trait]` / `#[ww_api_root]` emit a Rust trait of the same name and visibility that describes the API for
  rustdoc (DOC-6), instead of private `SrcMarker*` aliases. It is checked by the compiler in the API crate, so
  definitions that slipped through before fail now: types must be valid Rust (`RefVec<'i, &'i str>` instead of
  `RefVec<'i, str>`, `I2cReadEnvelope<'i>` instead of a hidden lifetime), and a trait used from another crate in
  `ww_impl!` must be `pub` (it was enough for its `*_FULL_GID` const to be public). Such a crate must be built with
  this version too, since `ww_impl!` now names the trait itself. Wire format and codegen output are unchanged.
- `ww_api!` is renamed to `ww_codegen!` (the old name is deprecated). `client = ".."` flavors are now
  `"std_client"` (from the API root, transport picked at connect time) and `"trait_client"`; `"raw"` (no_std) is
  not working yet.

### 🚀 Features

- `ww_codegen!(crate :: Api, local_trait = "Name")` emits the API as an async Rust trait `Name` (and `NameDyn` for trait objects) in the API crate itself, `impl_local_trait = "api::Name"` implements it on the `std_client` (CLT-57, `docs/api/local_trait.md`). `for <struct>` is optional when only `local_trait` is generated.
- `ww_codegen!` / `ww_impl!` name types of a crate the calling crate doesn't depend on through a crate it does depend on that re-exports it (API-60, `wire_weaver_core::load_dep_with_paths`): a server depending on `ww_si` alone compiles when `ww_si`'s types come from `exact_units`.
- The rustdoc trait of `#[ww_trait]` notes `#[claim]`, `#[shares]` and `#[label]` of an item or trait (SRV-34).
- `ww_codegen!(.., handler_model = "device.settings.*=async, _=sync")` (SRV-21): which handlers of an async server are plain `fn`s, see `docs/api/handlers.md`.
- `ww_codegen!(.., observe_cells = "root_foc, ..")` (API-50, observe cells): levels with `+batched` properties
  sampled in place through a ring of atomics, see wire_weaver_core.

- The rustdoc trait of `#[ww_trait]` notes batched streams (`batched`, `#[batched]`, API-41).

- `ww_codegen!(.., const_model = "SERIAL|BOARD=fn")` (API-36): which `const` items the server provides as a `fn` of
  the generated `<RootTrait>Consts` trait instead of a Rust const (`_=const` by default), see
  `docs/api/properties.md`.
- `#[ww_trait]` accepts `#[stream(T, kind)]` on trait fns and `#[counted]` / `#[reliable]` on `stream!` (API-31), and
  notes them in the rustdoc trait.
- `#[errors(..)]` on methods of a `#[ww_trait]` (API-32): a wrong list is a compile error pointing at the name in it
  (`CommonError has no variant Bogus, it has: ..`), and the rustdoc trait gets a `# Errors` section with each listed
  variant's doc line.
- `#[errors(..)]` on stream functions and on `property!` items (API-32), with the same compile errors and `# Errors`
  section in the rustdoc trait.
- API traits show up in `cargo doc` and on docs.rs (DOC-6): methods with their signatures, properties, streams,
  sinks, consts and `ww_impl!` sub-traits as functions returning `wire_weaver::api_doc` markers, all with their doc
  comments plus a note with access mode, array index, `#[credit]` and `#[since]`. The trait doc comment moved from
  `*_FULL_GID` to the trait, the `*_FULL_GID` / `*_COMPACT_GID` consts got docs of their own.
- `omit = ["i2c.pullups", ..]` and `omit_if(<cfg>) = [..]` arguments of `ww_codegen!` (API-16): `#[optional]`
  items (or groups) a server leaves out, always or when the cfg predicate holds (`omit_if(not(feature = "pullups"))`).
- `medium = "path::to::Medium"` argument of `ww_codegen!`, the type handlers get from `cx.medium()`, see
  `wire_weaver_core` changelog for the handler context.
- `#[ww_api_root]` to mark the API entry point, `#[ww_trait(gid)]` to give a trait a global id.
- `compact_version!()` creates a `CompactVersion` at compile time.
- `#[ww_trait]` is fully parsed at the call site, so errors show up earlier.
- Generated code carries source markers with correct spans, to navigate from generated code back to the definition.
- `introspect = "no_docs" | "with_docs"` argument generates `INTROSPECT_BYTES` with the `ww_self` serialized API.

### 🐛 Fixes

- `ww_codegen!` called from a `macro_rules!` no longer warns `unused import` on its `_SOURCE_MARKER` re-export.
- Drop the unused `relative-path` dependency (P2401, cargo-machete).
- `ww_codegen!` names types the API uses by their module path (API-46): a type in a `pub mod` of the API crate works
  without a root re-export, a type re-exported under another name works, a marker `units::Volt` next to an alias
  `Volt` is no longer taken for the alias, and `use .. as ..` imports in API crates are followed.

- `ww_codegen!` docs and the error for an unknown `client` value list the real flavors (`std_client`,
  `trait_client`, `raw`) instead of `full_client` / `full_client+usb` / `async_worked`.
- `#[ww_trait]` methods with a `MaxLen<&'i str, N>` (any const generic) argument compile: the helper aliases only
  take lifetimes.
- Correct span for `ww_impl!` of a trait in the same crate.

### 📚 Documentation

- Docs: product names written WireWeaver and ShrinkWrap in prose.

## 0.4.0 - 07 Jan 2026

### 🚀 Features

- Property_model support with get_set and value_on_changed options.
- Method_model deferred passes seq number to method and uses Option<return ty> to determine whether to answer
  immediately or not.
- #[derive(ShrinkWrap)]
- Implement repr un enums.
- External types support in shrink_wrap attr.
- U1..=U63 support, #[fixed_size] and #[dynamic_size] attributes for derive_shrink_wrap attribute macro.
- #[owned = "feature"] attribute to generate TyOwned from Ty<'i> and serdes code for it.
- full_version!() proc macro that generates const FullVersion.
- ww_impl! proc macro that generates ww_trait server or client implementation in place.
- ww_trait support in separate files, multiple API levels.
- Convert ww_trait to const for ident collision checks and docs bypass.
- ww_trait: emit proper compiler error if lifetime on a referenced type is incorrect.
- Global trait addressing support
- Derive ShrinkWrap macro.

### 🐛 Bug Fixes

- Collect methods and streams doc comments.
- Always add ww_repr in wire_weaver_api macro
- Do not handle repr in derive_shrink_wrap macro
- Warnings of unused attributes, when they are in fact used.
- full_version path.
- Generate no_alloc code only when lifetimes are present and not based on types used
- Make whole API level owned when no_alloc = false
- Referenced type lifetime check name collision
- Generate connect client methods only if async_worker+usb is used
