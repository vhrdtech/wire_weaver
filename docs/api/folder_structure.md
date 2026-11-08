---
level: basic
---

# Folder structure

An API is shared by the device and every host program that talks to it, so it lives in a crate of its own:

![API crates diagram](../assets/api_crates.svg)

- **`my_device_api`**: the API trait and its data types, nothing else. Used by the firmware and the host, so it must
  build for `no_std`, with an optional `std` feature for owned types (see `examples/blinky_api`).
- **Firmware**: depends on the API crate and implements the server. WireWeaver generates the serdes and dispatch
  code, the firmware writes the handlers (see `examples_mcu/`).
- **Rust driver** (`my_device`): depends on the API crate with `std`. WireWeaver generates the client; a
  hand-written, higher-level driver can sit on top of it (see `examples/blinky`).
- **Applications**: CLIs, GUIs, test benches and scripts depend on the Rust driver. A typed Python package wraps the
  Rust driver with PyO3 (see `examples/blinky_py`); for quick scripts without generated code, the
  [Python](../python.md) module and the [`ww` CLI](../cli.md) build the API at runtime from what the device sends.

The [wire_weaver_template](https://github.com/vhrdtech/wire_weaver_template) repository has this shape ready to copy.

## The API crate's name and version

The name of the API crate (from `Cargo.toml`) identifies the API worldwide (it goes into `ww_version::FullVersion`,
the generated `<ROOT_TRAIT>_FULL_GID` constant), so publish it to crates.io for an open-source project, or pick a
name unique enough for internal use.

Its `Cargo.toml` holds two versions. The crate version is Cargo's: normal [SemVer rules](https://semver.org) for
the Rust API. The [wire version](../evolution/rules.md#the-wire-version) is what a host and a device decide on when
they connect: the same line talks, another line is told apart before the first call.

```toml
[package]
name = "my_device_api"
version = "0.1.0"

[package.metadata.wire_weaver]
wire = "w1.0"
```

What counts as a breaking change for each is in the [evolution rules](../evolution/rules.md); `ww api check` says
which bumps a change needs. More in [Versioning](versioning.md).
