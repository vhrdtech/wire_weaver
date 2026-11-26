# Compare wire formats

Encodes eight example messages with `shrink_wrap` (WireWeaver's own wire format) and eight other pure-Rust formats
(postcard, MessagePack, JSON, CBOR, wincode, protobuf, FlatBuffers, ASN.1 UPER) and reports each encoding's byte
count and how the format lets a field be added later: `schema_rules`, `self_describing`, `extension_markers` or
`none`. The book page [wire sizes compared](../../docs/examples/wire_sizes.md) explains the settings and shows the
results.

The messages are in `src/msgs/`, one module each: an RPC request, a sensor sample, a log record, a firmware chunk,
a device configuration, a GPS fix, a CAN frame batch and a state report. Each module has the message as a plain
Rust type (read by `shrink_wrap` without layout directives and by every `serde` format), the same message with
`shrink_wrap` layout directives, and hand-written protobuf and ASN.1 types. Where `fixint` makes postcard smaller,
the module also has a tuned postcard type.

`cargo run` prints a transcript; `docs/generated/compare_wire_formats/` holds one table and one transcript per
message, `summary.csv`, `winners.csv` and the chart `wire_sizes.svg` (each format's mean size over the smallest
encoding, grouped by how it evolves), kept byte-for-byte by `book_output::check` (stale: `WW_BOOK_OUTPUTS=write
cargo run`, or `just book-outputs` from the repository root). FlatBuffers' Rust bindings are generated from
`schema/request.fbs` and `schema/messages.fbs` (`planus rust -o src/fbs_request.rs schema/request.fbs` and
`planus rust -o src/fbs_messages.rs schema/messages.fbs`, `cargo install planus-cli`) and committed; the rest need no code generation. This crate is excluded from the main
workspace (its own `Cargo.lock`) so these formats never reach every other build's lockfile; see `just`'s
`book-outputs` and `test-all` recipes for how it is still run and checked.
