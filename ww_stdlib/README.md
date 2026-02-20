# ww_stdlib

> [WireWeaver](https://github.com/vhrdtech/wire_weaver) standard library of types and global traits

## Overview

### Foundational types

* `ww_version` - SemVer version (including pre and build strings).
* `ww_date_time` - ISO 8601 date and time with optional time zone and nanoseconds, as small as 32 bits. Also NaiveDate
  and NaiveTime.
* `ww_numeric` - Various numeric types, including offset-scale and subtypes.
* `ww_si` - SI units and derived values.

### Common firmware functionality

* `ww_dfu` - Firmware update data types and API.
* `ww_counters` - Event counters API.
* `ww_log_bare_metal` - Logging types and API for no_std bare metal targets.
* `ww_uid` - Device unique ID data types and requesting API.
* `ww_board_info` - PCB/PCBA information (name, revision, etc.) and requesting API.
* `ww_gpio` - GPIO control data types and remote bridging API.
* `ww_can_bus` - CAN bus data types and remote bridging API.
* `ww_spi` - SPI data types and remote bridging API.
* `ww_uart` - UART data types and remote bridging API.
* `ww_i2c` - I2C bus data types and remote bridging API.
* `ww_indication` - Device indication LED control API.

### Meta

* `ww_client_server` - Client-server API model data types for both no_std and std.
* `ww_self` - Dynamic access to APIs (API model AST in shrink_wrap format).
* `ww_global` - ID registry, [more on addressing](https://vhrdtech.github.io/wire_weaver/api/addressing).

## What is a global trait?

Global trait is a WireWeaver API code-generator concept, using crates.io as a source of truth for commonly used
API functionality.

Each crate contains data types common to firmware and host, API definition and optional arbitrary Rust code
that can work only within the local API definition. As an example see `ww_gpio`, which contains a "low-level"
API, that will be code-generated at compile time and a "high-level" API providing better ergonomics and static types.