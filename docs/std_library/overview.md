---
level: basic
---

# Overview

`ww_stdlib` is a set of crates with ready-made WireWeaver traits and data types: GPIO, UART, I2C, CAN, logging,
firmware info, firmware updates, physical units and more. Instead of designing its own GPIO or logging API, a device
picks the crates it needs and puts their traits into its API, and a host (driver, CLI, GUI) gets a matching typed
client from the same codegen.

```toml
[dependencies]
ww_gpio = "0.1"
ww_firmware_info = { version = "0.1", default-features = false }
```

```rust
#[ww_api_root]
pub trait MyBoard {
    ww_impl!(firmware: ww_firmware_info::FirmwareInfo);
    ww_impl!(gpio: ww_gpio::Bank);
    fn my_own_method(value: u8);
}
```

A crate's name and version are part of the wire contract (see [folder structure](../api/folder_structure.md)), so
using `ww_gpio` on both ends is what lets tools of unrelated projects recognize "this is a GPIO bank" without
agreeing on anything else: `ww info` reads the firmware info of any device that has it, wherever it sits in the API.

All crates are `no_std` by default; the `std` feature adds owned types (`..Owned`, `String` instead of `&str`) for
host code. Most of them live under `ww_stdlib/` in this repo (canonical home: `vhrdtech/ww_stdlib`); the framework
crates `ww_self` and `ww_global` live in the `wire_weaver` repo root, versioned the same way.

## Picking crates

Start from what the device has:

- Every device implements the [**device contract**](#device-contract): which firmware and board it is, which unit,
  its log and counters, so `ww info` and a support case have one place to look whatever the device does.
- Add the [**I/O**](#io) traits for the hardware the device exposes over the wire (a GPIO bank, a UART bridge, an I2C
  master, ...). Pick only what is bridged: a thermometer implements `Adc`, not `AdcStream` as well.
- Use the [**data types**](#data-types) in your own methods and types: a physical value is a `ww_si` quantity, not a
  raw integer with an implied unit, and an API's error enum nests `ww_error::CommonError` first.
- The [**framework**](#framework) crates are used by the codegen and introspection; an API crate rarely depends on
  them directly.

## I/O { #io }

Traits for devices that expose IO over the wire: a dev board bridging its pins to a host, a USB adapter, a motor
controller, a sensor node. The [IO trait family](io_family.md) page has the conventions they share (capabilities
queried at runtime, one `configure()` call per bus, common errors, typed `ww_si` values, counted streams).

* [`ww_gpio`](ww_gpio.md) — `Bank`, `Pin`: read, drive and reconfigure pins, edge events as a stream
* [`ww_uart`](ww_uart.md) — `Uart`: bridge a serial port, optionally watch a link without driving it
* [`ww_can_bus`](ww_can_bus.md) — `Can`, `CanGroup`, `CanTimeTriggered`, `CanXl`: CAN, CAN FD and CAN XL controllers,
  from simple bridges to bus debuggers
* [`ww_i2c`](ww_i2c.md) — `I2c`, `I2cWatch`, `I2cMonitor`: master transfers and bus recovery, autonomous polling, bus
  sniffing
* [`ww_spi`](ww_spi.md) — `Spi`: transfers with CS held across operations, optional bus sniffing
* [`ww_adc`](ww_adc.md) — `Adc`, `AdcStream`, `AdcCapture`: one calibrated reading, continuous streaming, triggered
  block capture
* [`ww_pwm`](ww_pwm.md) — `Pwm`, `PwmCapture`: PWM outputs, pulse and frequency measurement
* [`ww_onewire`](ww_onewire.md) — `OneWire`: reset, ROM search, addressed transactions, strong pull-up
* [`ww_timebase`](ww_timebase.md) — `Clock`: device time, clock sync (master/slave), time of day
* [`ww_logic_analyzer`](ww_logic_analyzer.md) — `LogicAnalyzer`, `LogicStream`: timestamped capture of many digital
  channels
* [`ww_signal_generator`](ww_signal_generator.md) — `PatternGenerator`, `WaveformGenerator`: digital pattern
  playback, DAC waveforms
* [`ww_memory`](ww_memory.md) — `Memory8`, `Memory32`: read and write words at addresses, optional watches, for
  register maps and raw memory access
* [`ww_block`](ww_block.md) — `BlockDevice`: read, CRC, erase and write an SD card, eMMC or flash by block; writes
  under a lease that stops the card's own user, optional bulk sink and read stream
* [`ww_sd`](ww_sd.md) — `SdCard`: an SD card's state, raw registers, init with a per-step report, raw commands with
  response, status bits and timing

## Device contract

The crates every device implements, whatever it does, so generic tools have one place to look. The
[device contract](device_contract.md) page shows where to put them and how a host finds the firmware's ELF.

* [`ww_firmware_info`](ww_firmware_info.md) — `FirmwareInfo` (name, version, git, build time, build id),
  `LastReset` (why the device last reset: fault record, stack words, boot count)
* [`ww_board_info`](ww_board_info.md) — `BoardInfo`: PCB series, revision and how the revision was determined
* [`ww_uid`](ww_uid.md) — `Uid`: the unit's unique ids (chip id, MAC, EUI-64, ...)
* [`ww_user_info`](ww_user_info.md) — `UserInfo`: a name and notes the user gives a unit
* [`ww_log`](ww_log.md) — `LogDefmt`, `LogText`: device logging over a reliable stream, raw `defmt` frames or text;
  `ww_defmt_logger` (repo root) is the device side
* [`ww_counters`](ww_counters.md) — `Counters`: the device's `cnt` counters, decoded on the host with the firmware's
  ELF (`ww counters`)
* [`ww_recorder`](ww_recorder.md) — `Recorder`, `Track`: control a logger that writes tracks to an SD card or flash:
  status, card facts, two-step format, recordings, markers, flush settings, counters
* [`ww_indication`](ww_indication.md) — `Indication`: LED and buzzer mode, brightness, identify
* [`ww_dfu`](ww_dfu.md) — `UpdateNode`, `BootControl`, `Inventory`, `Stage`, `StageBulk`: firmware updates, A/B or
  in place, step by step or bulk

## Data types

Types other crates and your own API build on.

* [`ww_si`](ww_si.md) — `Volt`, `Hertz`, `Second`, ...: physical values with their unit, in whatever number type
  the device picks
* [`ww_error`](ww_error.md) — `CommonError`, `HasCommon`: the errors every API can return (`Unsupported`, `InUse`,
  `Busy`, ...), nested first in a crate's own error enum
* [`ww_date_time`](ww_date_time.md) — `DateTime` (ISO 8601, optional time zone and nanoseconds, 32 bits minimum),
  `NaiveDate`, `NaiveTime`
* [`ww_numeric`](ww_numeric.md) — `NumericValue`, `NumericBaseType`, `NumericAnyType`: the number types `ww_si`
  quantities are stored as

## Framework

Used by the codegen and introspection; an API crate rarely depends on these directly.

* [`ww_version`](ww_version.md) — `Version`, `FullVersion` (SemVer, with pre and build strings), `CompactVersion`
  (a global type id and the version numbers): how an API is identified on the wire
* [`ww_client_server`](ww_client_server.md) — `Request`, `Event`, `Error`: the protocol every generated server and
  client speaks underneath a device's own API
* [`ww_self`](ww_self.md) — `ApiBundle`, `ApiLevel`, `Type`, `Value`: WireWeaver's model of an API and its types,
  for introspection and dynamic access (repo root, not `ww_stdlib/`)

`ww_global` (the global type id registry, see [addressing](../api/addressing.md)) is the other framework crate in the
`wire_weaver` repo root; it has no page of its own.

## Host-side `_hl` crates

Crates with a more ergonomic client on top of a generated one, for drivers, CLIs and GUIs rather than firmware.

* `ww_gpio_hl` — async, blocking and promise wrappers over `ww_gpio`'s generated client
