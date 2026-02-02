# Folder structure

Intended crate organisation is as follows:

![API crates diagram](../assets/api_crates.svg)

* `my_device_api` crate - contains user-defined types and resources, common to firmware, and it's driver (server and
  client).
  This crate must support `no_std` and optionally `std`.
* MCU firmware depends on the API crate, uses common data types and implements a server. WireWeaver generates
  serdes and dispatch code, while user provides actual implementation.
* Rust driver also depends on the API crate (optionally with std feature). WireWeaver generates serdes and client side
  code, user can optionally provide a higher-level client implementation on top of the generated one.
* CLI, GUI and other applications depend on the Rust driver crate in order to communicate with the device.
* TODO: Python wrapper is also automatically generated and uses Rust driver code.

Name of the API crate (from Cargo.toml) is assumed to be a globally unique identifier (see `ww_version::FullVersion`),
hence it is advised to eventually publish it to crates.io if you are working on an open-source project or ensure to use
unique enough name for internal use.

Version of the API crate is used for compatibility checks upon connection to the device. You can use it in your
code as well, to show proper messages to user when interacting with an older or newer firmware from the perspective of
the
driver. Normal [SemVer rules](https://semver.org) apply.
