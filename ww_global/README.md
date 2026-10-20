# Global id registry

![Crates.io Version](https://img.shields.io/crates/v/ww_global)

A request can call a reusable trait wherever it sits in a device's API, without knowing the device's API: it names the
trait instead of a path (`PathKind::GlobalFull`, the crate name, version and trait name). That takes 8 to 16 bytes or
more on the wire. For common traits this registry assigns a small number instead, and the request carries the id
and version numbers only (`PathKind::GlobalCompact`), 2 bytes and up. See
[resources addressing](https://vhrdtech.github.io/wire_weaver/api/addressing/).

## Rules

- One id per trait, keyed `crate::Trait`, in the same number space as the crate ids (a few crates have one too:
  `ww_log`'s id names its types in recorder tracks, link and protocol crates carry theirs in `CompactVersion`).
- Ids live only in `wire_weaver_gid.json` and are assigned by hand. Code never carries the number: the codegen
  looks a trait's id up by crate and trait name, so moving it to another module, reordering traits or adding a
  sibling trait changes nothing.
- A rename or a move to another crate keeps the id: the new name becomes the key, the old one goes into `aliases`.
- Ids are never reused: a removed trait stays in the file with `"retired": "<why, when>"`.
- `assigned_ids.lock` is the append-only record of every id ever assigned; after adding an id, run `just gid-lock`.
  A test fails when an id changes or vanishes, when a public `#[ww_trait]` in `ww_stdlib` has no id (or isn't in
  `no_id`), on duplicates and on an id in a reserved range.

## Ranges

Ids are `UNib32` on the wire, so small numbers are shorter: `0..=7` take one nibble, `8..=63` one byte, `64..=511`
1.5 bytes, `512..=4095` 2 bytes.

| Ids       | For                                                                                               |
|-----------|---------------------------------------------------------------------------------------------------|
| 0-7       | `ww_log` (crate) and `ww_log::LogDefmt` only; 2-7 stay free until real use shows a trait is requested globally a lot |
| 8-63      | the device contract and common IO: firmware and board info, identity, counters, firmware update, clock, indication, GPIO, UART, I2C, SPI, CAN, ADC, PWM |
| 64-511    | all other reusable traits                                                                         |
| 512+      | link and protocol crates                                                                          |

Each family takes a block with gaps left for related traits to come.

## Ids

| Id  | Kind  | Name |
|-----|-------|------|
|   0 | crate | `ww_log` |
|   1 | trait | `ww_log::LogDefmt` |
|   8 | trait | `ww_firmware_info::FirmwareInfo` |
|   9 | trait | `ww_firmware_info::LastReset` |
|  10 | trait | `ww_board_info::BoardInfo` |
|  11 | trait | `ww_uid::Uid` |
|  12 | trait | `ww_user_info::UserInfo` |
|  13 | trait | `ww_counters::Counters` |
|  14 | trait | `ww_log::LogText` |
|  16 | trait | `ww_dfu::BootControl` |
|  17 | trait | `ww_dfu::UpdateNode` |
|  18 | trait | `ww_dfu::Inventory` |
|  19 | trait | `ww_dfu::Stage` |
|  20 | trait | `ww_dfu::StageBulk` |
|  24 | trait | `ww_timebase::Clock` |
|  25 | trait | `ww_indication::Indication` |
|  32 | trait | `ww_gpio::Bank` |
|  33 | trait | `ww_gpio::Pin` |
|  36 | trait | `ww_uart::Uart` |
|  38 | trait | `ww_i2c::I2c` |
|  40 | trait | `ww_spi::Spi` |
|  42 | trait | `ww_can_bus::Can` |
|  44 | trait | `ww_adc::Adc` |
|  45 | trait | `ww_adc::AdcStream` |
|  48 | trait | `ww_pwm::Pwm` |
|  49 | trait | `ww_pwm::PwmChannel` |
|  64 | trait | `ww_memory::Memory8` |
|  65 | trait | `ww_memory::Memory32` |
|  72 | trait | `ww_onewire::OneWire` |
|  80 | trait | `ww_i2c::I2cWatch` |
|  81 | trait | `ww_i2c::I2cMonitor` |
|  88 | trait | `ww_can_bus::CanGroup` |
|  89 | trait | `ww_can_bus::CanTimeTriggered` |
|  90 | trait | `ww_can_bus::CanXl` |
|  96 | trait | `ww_adc::AdcCapture` |
|  97 | trait | `ww_pwm::PwmCapture` |
|  98 | trait | `ww_pwm::PwmCaptureChannel` |
| 104 | trait | `ww_block::BlockDevice` |
| 105 | trait | `ww_sd::SdCard` |
| 112 | trait | `ww_recorder::Recorder` |
| 113 | trait | `ww_recorder::Track` |
| 120 | trait | `ww_logic_analyzer::LogicAnalyzer` |
| 121 | trait | `ww_logic_analyzer::LogicStream` |
| 128 | trait | `ww_signal_generator::PatternGenerator` |
| 129 | trait | `ww_signal_generator::WaveformGenerator` |
| 130 | trait | `ww_signal_generator::WaveformChannel` |
| 136 | trait | `ww_pin_diag::PinDiag` |
| 512 | crate | `wire_weaver_usb_link` |
| 513 | crate | `ww_client_server` |

## File format

```json
{
  "reserved": [{ "ids": [2, 7], "reason": "..." }],
  "ids": [
    { "id": 0, "crate": "ww_log" },
    { "id": 1, "trait": "ww_log::LogDefmt" },
    { "id": 99, "trait": "new_crate::NewName", "aliases": ["old_crate::OldName"] },
    { "id": 100, "trait": "ww_x::Gone", "retired": "removed in ww_x 0.3 (20 Oct 2026)" }
  ],
  "no_id": ["ww_x::Internal"]
}
```

Each entry has an `id` and exactly one of `crate` or `trait`. `build.rs` turns the file into a const per crate entry
(`WW_LOG`, `WIRE_WEAVER_USB_LINK`, `WW_CLIENT_SERVER`) and the `ENTRIES` table, with `trait_id`, `crate_id` and
`entry` lookups for codegen and tools.

If you create a generic trait that is useful on many devices, open a PR with an id for it.
