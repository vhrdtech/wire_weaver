# standalone_units

A crate not built on `wire_weaver` (API-60: types-from-crates-that-arent-api-crates): plain data types with no
`wire_weaver` dependency and no `#[ww_trait]`, `shrink_wrap` only as an optional dependency behind the
`shrink_wrap` feature, the way `exact_units` is.

Used by `tests/standalone_api`, which turns the `shrink_wrap` feature on so the crate walker reads these types.
