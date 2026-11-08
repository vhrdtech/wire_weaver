---
level: advanced
---

# Versioning

An API's version is its crate's version, and the same goes for every trait and type: a `#[ww_trait]` or a data type
from another crate is identified by that crate's name and version. `ww_version::FullVersion` carries both; codegen
puts the API root's into a `<ROOT_TRAIT>_FULL_GID` constant (`BLINKY_API_FULL_GID`), which the device reports when a
host connects.

The crate version is for Cargo; devices and hosts decide on the API root's
[wire version](../evolution/rules.md#the-wire-version), `[package.metadata.wire_weaver] wire = "w2.3"` in the API
crate's `Cargo.toml`, which codegen puts into a
`<ROOT_TRAIT>_WIRE_VERSION` constant next to the full id. On connection, a generated client checks the device's API
crate name and wire line against its own and refuses another API or line with `Error::IncompatibleDeviceProtocol`;
the device does the same with the host's. A crate breaking bump on the same wire line connects. Within a line the
[evolution rules](../evolution/rules.md) keep old and new talking: added methods, appended fields, new enum
variants. The dynamic client, the `ww` CLI and Python take the device's API from its introspection data, so they
don't check.

A device passes both to `ww_device::LinkConfig::new(BLINKY_API_FULL_GID, BLINKY_API_WIRE_VERSION, ..)`; a generated
client sets its own with `ClientConfig::client_version(version, wire)`.

Reusable traits are found by their crate's name and version too, whatever the device's own API is, see
[global trait addressing](addressing.md#global-trait-addressing-fullversion). The types are on the
[ww_version](../std_library/ww_version.md) page.
