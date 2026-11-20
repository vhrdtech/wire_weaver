# layers_lint

Tests only (R9, R1): fails on any workspace crate missing from `layers.toml`, on a dependency edge from a
lower-layer crate to a higher one that `layers.toml`'s `[[allow]]` list doesn't name, on an API crate reaching a
runtime crate, and on a workspace member without a `[lints]` table.

See `tests/layer_lint.rs`, `/layers.toml` and `docs/evolution/crate_layers.md`.
