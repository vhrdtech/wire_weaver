## Unreleased

### ⚠️ Breaking

- `ww_api!` is renamed to `ww_codegen!` (the old name is deprecated). `client = ".."` flavors are now
  `"full_client"`, `"full_client+usb"` and `"trait_client"`.

### 🚀 Features

- `medium = "path::to::Medium"` argument of `ww_codegen!`, the type handlers get from `cx.medium()`, see
  `wire_weaver_core` changelog for the handler context.
- `#[ww_api_root]` to mark the API entry point, `#[ww_trait(gid)]` to give a trait a global id.
- `compact_version!()` creates a `CompactVersion` at compile time.
- `#[ww_trait]` is fully parsed at the call site, so errors show up earlier.
- Generated code carries source markers with correct spans, to navigate from generated code back to the definition.
- `introspect = "no_docs" | "with_docs"` argument generates `INTROSPECT_BYTES` with the `ww_self` serialized API.

### 🐛 Fixes

- Correct span for `ww_impl!` of a trait in the same crate.

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
