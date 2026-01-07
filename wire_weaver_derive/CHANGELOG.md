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
