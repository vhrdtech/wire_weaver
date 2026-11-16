## Unreleased

### 🚀 Features

- New crate (API-68): the API crate prelude, free of the runtime. `ww_api::prelude` has the `#[ww_trait]`, `#[ww_api_root]`, `ww_impl!`, `ww_codegen!`, `full_version!` and `compact_version!` macros, `shrink_wrap` and its prelude, `ww_error` (`CommonError`, `HasCommon`, `CommonErrorOwned` with `std`) and `ww_version` (`FullVersion`); the crate root re-exports the same. An API crate depends on `ww_api` instead of `wire_weaver`: `use ww_api::prelude::*;`. Features `std`, `defmt` and `serde` forward to the re-exported crates.
