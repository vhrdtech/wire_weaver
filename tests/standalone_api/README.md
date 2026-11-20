# standalone_api

Test API trait crate built on `standalone_units`' types, exercising a dependency that isn't itself a
`wire_weaver` API crate (API-60: types-from-crates-that-arent-api-crates).

Re-exports `standalone_units` so a crate depending on this one names its types without its own dependency. Used
by `tests/standalone`.
