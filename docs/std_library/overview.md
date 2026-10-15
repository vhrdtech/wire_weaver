# Overview

`ww_stdlib` is a set of crates that put reusable WireWeaver traits and data types on crates.io instead of each
project reinventing its own GPIO, logging or firmware-info API: a device picks the ones it needs and puts each
into its API tree with [`ww_impl!`](../api/traits.md), and a host (driver, CLI, GUI) gets a matching, typed client
for free from the same codegen. A crate's name and version are part of the wire contract (see
[folder structure](../api/folder_structure.md)), so pulling in `ww_gpio` on both ends is what lets unrelated
projects' tools recognise "this is a GPIO bank" without agreeing on anything else.

Most crates are listed as part of `ww_stdlib/` in this repo (canonical home: `vhrdtech/ww_stdlib`); a few
framework-level ones (`ww_self`, `ww_global`) live in the `wire_weaver` repo root instead, versioned the same way.
All of them are `no_std` by default; the `std` feature adds owned types (`..Owned`, `String` instead of `&str`) for
host-side code.

## Picking crates

Start from what the device actually has:

- Every device implements the **device contract** crates — `ww_firmware_info`, `ww_board_info`, `ww_uid`,
  `ww_user_info`, and `ww_log` for logging — so `ww list` and a support case have one place to look, regardless of
  what the device does.
- Add **IO family** traits matching the hardware exposed over the wire (a GPIO bank, a UART bridge, an I2C master,
  ...). Pick only what is actually bridged; a thermometer implements `Adc`, not `AdcStream` as well.
- Reach for **types** crates (`ww_si`, `ww_numeric`, `ww_date_time`, `ww_error`) when defining your own methods and
  types: physical values go through `ww_si`, not a raw integer with an implied unit, and a crate's own error enum
  nests `ww_error::CommonError` first.
- The **framework** crates (`ww_client_server`, `ww_self`, `ww_version`) are pulled in by the codegen and
  introspection machinery; a device API crate rarely depends on them directly.
- Host-only tooling (a CLI, a GUI, a Rust driver) can depend on a **`_hl`** crate (e.g. `ww_gpio_hl`) for an
  ergonomic client on top of the generated one, instead of talking to the generated client directly.

## IO family

Traits for devices that expose IO over the wire — a dev board bridging its pins to a host, a USB adapter, a motor
controller, a sensor node. See [IO trait family](io_family.md) for the shared conventions (capabilities queried at
runtime, one `configure()` call per bus, common errors, typed `ww_si` values, counted streams).

* [`ww_gpio`](ww_gpio.md) — `Bank`, `Pin`: logical-pin GPIO, read/drive/reconfigure, edge events as a stream
* [`ww_uart`](ww_uart.md) — `Uart`: bridge a serial port, optional line monitoring without driving it
* [`ww_i2c`](ww_i2c.md) — `I2c`, `I2cWatch`, `I2cMonitor`: master transfers and bus recovery, autonomous polling, bus sniffing
* [`ww_spi`](ww_spi.md) — `Spi`: master transfers with CS held across operations, optional bus sniffing
* [`ww_adc`](ww_adc.md) — `Adc`, `AdcStream`, `AdcCapture`: one calibrated reading, continuous streaming, triggered block capture
* [`ww_pwm`](ww_pwm.md) — `Pwm`, `PwmCapture`: PWM channel outputs, pulse/frequency measurement on capture-capable channels
* [`ww_onewire`](ww_onewire.md) — `OneWire`: reset, ROM search, addressed transactions, strong pull-up
* [`ww_timebase`](ww_timebase.md) — `Clock`: device time, clock sync (master/slave), time-of-day for a master
* [`ww_logic_analyzer`](ww_logic_analyzer.md) — `LogicAnalyzer`, `LogicStream`: timestamped capture of many digital channels, triggered or streamed
* [`ww_signal_generator`](ww_signal_generator.md) — `PatternGenerator`, `WaveformGenerator`: digital pattern playback, DAC waveforms
* [`ww_can_bus`](ww_can_bus.md) — `CANBusInterfaceGroup`, `CANBusInterface`: CAN controller setup and per-interface TX/RX, termination, bus performance

## Device contract

Crates every device implements regardless of what it does, so generic tooling (`ww list`, a support case) has one
place to look.

* [`ww_firmware_info`](ww_firmware_info.md) — `FirmwareInfo` (name, version, git, build time, build id),
  `LastReset` (why the device last reset: fault record, stack words, boot count)
* [`ww_board_info`](ww_board_info.md) — `BoardInfo`: PCB series, revision and how the revision was determined
* [`ww_uid`](ww_uid.md) — `Uid`: the unit's unique ids (chip id, MAC, EUI-64, ...), one trait carrying several
* [`ww_user_info`](ww_user_info.md) — `UserInfo`: a name and notes the user gives a unit, with persistence rules
* [`ww_log`](ww_log.md) — `LogDefmt`, `LogText`: device logging over a reliable stream, raw `defmt` frames or text
  * `ww_defmt_logger` (repo root, device side) — the `defmt` global logger that feeds `LogDefmt::frames`, with an optional tee to RTT
* [`ww_counters`](ww_counters.md) — `Counters`: the device's `cnt` counters as raw buffer words (RAM and backup),
  decoded on the host with the firmware's ELF found by its build id (`ww counters`)
* `ww_indication` — `Indication`: device indication LED control (mode, brightness, test mode)
* [`ww_dfu`](ww_dfu.md) — `UpdateNode`, `BootControl`, `Inventory`, `Stage`, `StageBulk`: firmware update, A/B or
  in-place, step by step or bulk

## Debug / low level

* [`ww_memory`](ww_memory.md) — `Memory8`, `Memory32`: read/write words at addresses, optional watches, for
  register maps and raw memory access

## Types

Data types other crates build their wire format on.

* [`ww_si`](ww_si.md) — `Quantity` and the SI/derived units (`Volt`, `Hertz`, `Second`, ...) every physical value
  on the wire uses, instead of a raw integer with an implied unit
* [`ww_numeric`](ww_numeric.md) — `NumericValue`, `NumericBaseType`, `NumericAnyType`: the discrete/floating/fixed-point
  number types `ww_si` quantities are stored as, plus subtype and shift-scale
* [`ww_date_time`](ww_date_time.md) — `DateTime` (ISO 8601 with optional time zone and nanoseconds, 32 bits minimum),
  `NaiveDate`, `NaiveTime`
* [`ww_error`](ww_error.md) — `CommonError`, `HasCommon`: the errors every API can return (`Unsupported`, `InUse`,
  `Busy`, ...), nested first in a crate's own error enum as `#[common] Common(CommonError)`

## Framework

Framework-level crates the codegen and introspection machinery use; an API crate rarely depends on these directly.

* [`ww_client_server`](ww_client_server.md) — `Request`, `RequestKind`, `Event`, `EventKind`, `Error`: the
  client-server API model (the "meta-API" every generated server/client speaks)
* [`ww_self`](ww_self.md) — `ApiBundle`, `ApiLevel`, `Type`, `Value`: WireWeaver's own AST describing an API and its
  data types, for dynamic access, expression eval and introspection. Lives in the `wire_weaver` repo root, not
  `ww_stdlib/`
* [`ww_version`](ww_version.md) — `Version` / `FullVersion` (SemVer, with pre/build strings), `CompactVersion`
  (global type id + major/minor, for the wire)

`ww_global` (the global type ID registry, see [addressing](../api/addressing.md)) is the other framework-level
crate living in the `wire_weaver` repo root; it has no doc page of its own.

## Host-side `_hl` crates

Crates building a more ergonomic, statically-typed client on top of a generated one, for drivers/CLIs/GUIs rather
than firmware.

* `ww_gpio_hl` — async/blocking/promise wrappers over `ww_gpio`'s generated client
