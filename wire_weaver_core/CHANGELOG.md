## Unreleased

### ⚠️ Breaking

- Generated client `write_<property>` returns `PreparedWrite<UserError>` (`PreparedWrite<()>` without one) instead of
  `PreparedWrite<Result<(), UserError>>`, see `wire_weaver_client` changelog.

- Generated servers follow `ww_client_server`'s seq change: `Request::seq` is `UVlq32Backfill` and `Event::seq` is
  `UVlq32`, deferred `*_ser_return_event()` methods take a `u32` seq. Wire-incompatible with clients and devices built
  before this change.
- The API hash (`API_HASH_NO_DOCS`/`API_HASH_WITH_DOCS`) is calculated over the introspection data as a device sends
  it, with known traits and types left out, so it changes for APIs using `ww_stdlib` traits or types. A device and a
  client built with `wire_weaver_core` versions that know different snapshots report different hashes for the same
  API, which falls back to the per-resource compatibility check.
- The API hash covers `ww_self_version`, which now follows the `ww_self` crate version: a device and a client built
  with different `ww_self` versions report different hashes for the same API, with the same fallback.
- `#[flag] name: bool` fields are recorded as `TypeOwned::Flag` in introspection data and snapshots, they were a
  plain `bool` and didn't say that the `Option` or `Result` field `name` has no flag of its own, so dynamic clients
  misread such types. Introspection data, API hashes and signatures of types with relocated flags change
  (`ww_version::Version` and `FullVersion`, `ww_si::SIExp` and the types containing them). A device and a client
  built on different sides of this change report such types as incompatible (`bool` vs `flag`).

### 🚀 Features

- `load_crate()` loads all `#[ww_trait]`/`#[ww_api_root]` traits and `#[derive_shrink_wrap]` types defined in a crate
  into an `ApiBundleOwned`, with traits and types from other crates replaced by `SkippedFullVersion` references
  carrying the signature of the left out definition. Types re-exported from the crate's modules with `pub use` are
  included too. Used by `ww api save` to save crate snapshots.

- Introspection data sent by a device leaves out traits and types known from the snapshots embedded in
  `wire_weaver_snapshots` (`ww_global` and `ww_stdlib` crates), referring to them by crate version, name and signature
  instead (e.g. `examples/all_gpio_api` goes from 4363 to 72 bytes). Only definitions identical to the snapshot are
  left out. `transform::skip_known()` does it. Generated clients embed their own API in the same form.

- Server codegen emits `API_HASH_NO_DOCS`, `API_HASH_WITH_DOCS` and the compile-time `API_ID` string for USB
  identity strings.

- `gen_server_scaffold()` with `ServerScaffoldConfig` generates the user side of a server as Rust source: a server
  struct (with fields for `value_on_changed` properties), a stub for every handler the server codegen expects,
  with the resource's doc comments, and the matching `ww_codegen!` invocation. Method, getter and setter stubs return
  `Unimplemented`, valid indices stubs allow no index. Used by `ww api scaffold`.

### 🐛 Fixes

- `method_model = "..=deferred"` servers compile again: generated `<method>_ser_return_event` used an undefined
  `request` for unit methods, `RefVec` instead of `TailBytes` for the return value and `Error` without a lifetime.

- Types and traits can be referred to through modules: `mod ty; use ty::Ty;`, `crate::`/`self::` paths, and paths
  longer than `ext_crate::Ty` (e.g. `ext_crate::module::Ty`). `ww_version` and `shrink_wrap` used through
  `wire_weaver`'s re-exports (e.g. `use wire_weaver::prelude::*;`) resolve without a direct dependency on them.
  All of these failed with "Dependency not found" or "Only support `MyType` and `ext_crate::MyType`".
- Generated client methods no longer declare an unused 128-byte `args_scratch` buffer when `no_alloc` is not set.

- Loading a self-referential type (e.g., containing `RefBox<Self>`) fails with an error instead of overflowing the
  stack.

- `#[default = ..]` on fields of API types accepts any expression (e.g. `#[default = None]` as in the docs), not only
  string literals, which failed to load. Only the presence of a default is recorded in introspection data for now.

- Generated std client sets `client_version` to the API crate's `<TRAIT>_FULL_GID`, so the host and device check
  version compatibility during link setup instead of treating the client as dynamic.

## 0.4.0 - 07 Jan 2026

### 🚀 Features

- Property_model support with get_set and value_on_changed options.
- Method_model deferred passes seq number to method and uses Option<return ty> to determine whether to answer
  immediately or not.
- Serialize return value for deferred methods.
- Harden USB link implementation.
- #[derive(ShrinkWrap)]
- Implement repr un enums.
- External types support in shrink_wrap attr.
- U1..=U63 support, #[fixed_size] and #[dynamic_size] attributes for derive_shrink_wrap attribute macro.
- I2, I3, .., I63 support.
- Propagate ident spans, among other things makes output look better in IDE
- #[owned = "feature"] attribute to generate TyOwned from Ty<'i> and serdes code for it.
- Subtype scaffold.
- Replace #[final_evolution] with #[final_structure], add #[self_describing] and #[sized] attributes, implement const
  asserts.
- Handle #[default = None] on evolved types.
- ww_impl! proc macro that generates ww-trait server or client implementation in place.
- ww_trait support in separate files, multiple API levels.
- Implement resource array support for methods, properties, streams and API traits.
- Implement RefBox<'i, T>
- Check that flag order is LIFO.
- Property access mode.
- ww_si, ww_can_bus, ww_numeric and other library types
- Trait client structs
- Use the index chain in client codegen as well.
- Tuple and array support.
- Ww_trait: emit proper compiler error if lifetime on a referenced type is incorrect.
- Const properties
- Global trait addressing support
- Client: split methods into 2 - one with default timeout and the second with explicit value.
- Treat [T] and &[T] as Vec/RefVec<T> in API
- Stream sideband channel, document ww_client_server
- Partial stream client support
- Stream client subscribe
- Methods and streams tests
- Trait array server, client and integration test
- Support super and crate in ww_api macro.
- Error sequence ID in generated server code to help identify exact errors
- Blocking client for methods
- Generate connect_raw() and connect_raw_blocking()
- Reserved resource kind
- Scaffold introspect
- Stream and array of streams data serializers at any depth

### 🐛 Bug Fixes

- *(api)* Support derive with paths.
- User types returned from methods directly.
- Option in argument position.
- Collect methods and streams doc comments.
- Use ww_repr instead of repr for clarity, generate discriminant fn in ww_repr attr macro.
- Always add ww_repr in wire_weaver_api macro
- Treat strings as Unsized
- Handle Unsized in Option and Result
- Automatically switch to Owned type in client codegen if no_alloc == false, and add a use statement.
- Multi level traits
- Make whole API level owned when no_alloc = false
- Generate connect client methods only if async_worker+usb is used
- Array of streams on client
