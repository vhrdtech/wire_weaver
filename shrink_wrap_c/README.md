# shrink_wrap_c

> Builds the shrink_wrap C99 runtime and checks it against the Rust crate (SW-43)

Not published: `publish = false`, the C files (`shrink_wrap.h`, `shrink_wrap.c`, built next to this crate) are
what C users take, not this crate itself. Depends on the `shrink_wrap` crate to run the same sequence of
operations through both sides: golden vectors (`tests/golden.rs`) and a property-based cross-check in both
directions (`tests/cross.rs`). `CReader` and `CWriter` are thin safe Rust wrappers over the C API, and `model`
runs the same `Op` sequence through the Rust and the C side.

See [docs/c.md](https://github.com/vhrdtech/wire_weaver/blob/master/docs/c.md) for the C runtime's own
documentation.
