---
level: advanced
---

# Versioning

An API's version is its crate's version, and the same goes for every trait and type: a `#[ww_trait]` or a data type
from another crate is identified by that crate's name and version. `ww_version::FullVersion` carries both; codegen
puts the API root's into a `<ROOT_TRAIT>_FULL_GID` constant (`BLINKY_API_FULL_GID`), which the device reports when a
host connects.

On connection, a generated client checks the device's API crate name and version against its own and refuses an
incompatible device with `Error::IncompatibleDeviceProtocol`. Compatible means SemVer-compatible: the same major
version from 1.0 on, the same major and minor before it (`Version::is_protocol_compatible`). Within that range the
[evolution rules](../evolution/rules.md) keep old and new talking: added methods, appended fields, new enum
variants. The dynamic client, the `ww` CLI and Python take the device's API from its introspection data, so they
don't check.

Reusable traits are found by their crate's name and version too, whatever the device's own API is, see
[global trait addressing](addressing.md#global-trait-addressing-fullversion). The types are on the
[ww_version](../std_library/ww_version.md) page.
