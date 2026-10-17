# ww_stdlib

> [WireWeaver](https://github.com/vhrdtech/wire_weaver) standard library of types and global traits

## Overview

### Foundational types

* `ww_version` - SemVer version (including pre and build strings).
* `ww_date_time` - ISO 8601 date and time with optional time zone and nanoseconds, as small as 32 bits. Also NaiveDate
  and NaiveTime.
* `ww_numeric` - Various numeric types, including offset-scale and subtypes.
* `ww_si` - SI units and derived values.
* `ww_error` - Errors every API can return (`CommonError`), recognised by tools by their type.

### Common firmware functionality

* `ww_dfu` - Firmware update data types and API.
* `ww_counters` - The device's `cnt` counters over WireWeaver, decoded on the host with the firmware's ELF.
* `ww_recorder` - Control a recorder (a logger writing many tracks to an SD card or flash): status, format, recordings, markers, settings.
* `ww_log` - The device's log: defmt frames (decoded with the ELF) or text records, as reliable streams.
* `ww_uid` - Device unique ID data types and requesting API.
* `ww_board_info` - PCB/PCBA information (name, revision, etc.) and requesting API.
* `ww_gpio` - GPIO control data types and remote bridging API.
* `ww_can_bus` - CAN bus API: configure, send and receive frames, error state, optional filters and buffers.
* `ww_spi` - SPI data types and remote bridging API.
* `ww_uart` - UART data types and remote bridging API.
* `ww_i2c` - I2C bus data types and remote bridging API.
* `ww_memory` - Low-level access to memory or registers (8- and 32-bit), with optional watches.
* `ww_block` - Block device (SD card, eMMC, flash): read, device-side CRC, erase and write under a lease, optional bulk transfers.
* `ww_sd` - Raw SD card access: state, registers, init report, raw commands with response, status bits and timing.
* `ww_indication` - Device indication LED control API.

### Meta

* `ww_client_server` - Client-server API model data types for both no_std and std.

`ww_self` (API model AST for introspection) and `ww_global` (ID registry) are part of the framework itself and live in
the [WireWeaver](https://github.com/vhrdtech/wire_weaver) repo root.

## What is a global trait?

Global trait is a WireWeaver API code-generator concept, using crates.io as a source of truth for commonly used
API functionality.

Each crate contains data types common to firmware and host, API definition and optional arbitrary Rust code
that can work only within the local API definition. As an example see `ww_gpio`, which contains a "low-level"
API, that will be code-generated at compile time and a "high-level" API providing better ergonomics and static types.