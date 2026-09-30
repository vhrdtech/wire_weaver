## Unreleased

### 🚀 Features

- `load_crate()` loads all `#[ww_trait]`/`#[ww_api_root]` traits and `#[derive_shrink_wrap]` types defined in a crate
  into an `ApiBundleOwned`, with traits and types from other crates replaced by `SkippedFullVersion` references
  carrying the signature of the left out definition. Used by `ww api save` to save crate snapshots.

- Server codegen emits `API_HASH_NO_DOCS`, `API_HASH_WITH_DOCS` and the compile-time `API_ID` string for USB
  identity strings.

### 🐛 Fixes

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
