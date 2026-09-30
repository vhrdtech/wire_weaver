## Unreleased

### 🚀 Features

- `api_id` module: `ww:<crate>@<version> h=<hash>[ l=<label>]` identity string for USB interface descriptors, so
  hosts can identify a device without opening it. `api_id_string!` builds it at compile time, `with_label()` appends
  a runtime user label within the 126-character USB string limit, `parse()` reads it back (all `no_std`, no alloc).

## [0.4.0] - 07 Jan 2026

### 🚀 Features

* Re-export:
    * shrink_wrap
    * wire_weaver_derive
    * ww_version
