# thermo_api

The API of the multi-line bench (FEATURES.md EVO-28/EVO-42: a-host-for-several-wire-lines,
the-multi-line-bench-on-one-crate): a small temperature logger with every wire line still in use, one API root
per line, each wire version in `Cargo.toml`'s `[package.metadata.wire_weaver.roots]`.

Touches every place a wire break can bite: methods, properties, an enum, a stream and a sink, an array of
sub-levels, items added within a line, and unit types.
