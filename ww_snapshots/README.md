# ww_snapshots

API snapshots of WireWeaver crates, found by crate name and version.

A device leaves the traits and types known from other crates' snapshots out of its introspection data and names each by crate, version, name and signature. Codegen leaves one out only when the dependency's own `api_snapshots/` has the same definition. A host puts them back through a chain: sets the application embedded, the cache `~/.wire_weaver/snapshots/`, Cargo's sources on this machine, a fetch from crates.io or the crate's git repository (feature `fetch`), else the value stays opaque. Every file is checked against its header's SHA-256 and its crate and version, and a definition is put back only when its signature matches what the device sent.

No snapshot of any other crate is compiled into this crate. Docs: the WireWeaver book, chapter Evolution.
