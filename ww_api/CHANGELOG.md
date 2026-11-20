## Unreleased

### 🐛 Fixes

- `api_error!`'s doc example compiles: the error enum borrowing a field names its lifetime (`SensorError<'i>`), with its owned type and a second, lifetime-free error enum showing the macro's other form.

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.
- The prelude has no `HasCommon` (STD-16); every API error enum declares `api_error!(XError)` so handlers can `?` it into `Error<'_, XError>`.

### 🚀 Features

- `api_doc::Keys<E>`: the rustdoc marker of a keyed array's key companion (API-67).
- `Error<'i, E>` (STD-16): what a handler answers instead of a value, `Common(CommonError)` or `Api(E)`, with `From<CommonError>`; `api_error!(XError<'i>)` adds `From<XError>` for it in the error enum's crate (a blanket `From<E>` would overlap at `E = CommonError`). Both in the prelude. Generated servers take it from the next codegen on.
- New crate (API-68): the API crate prelude, free of the runtime. `ww_api::prelude` has the `#[ww_trait]`, `#[ww_api_root]`, `ww_impl!`, `ww_codegen!`, `full_version!` and `compact_version!` macros, `shrink_wrap` and its prelude, `ww_error` (`CommonError`, `HasCommon`, `CommonErrorOwned` with `std`) and `ww_version` (`FullVersion`); the crate root re-exports the same. An API crate depends on `ww_api` instead of `wire_weaver`: `use ww_api::prelude::*;`. Features `std`, `defmt` and `serde` forward to the re-exported crates.
