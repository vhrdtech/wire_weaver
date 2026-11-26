---
level: basic
---

# Examples and Templates

To start a new project, use the [wire_weaver_template](https://github.com/vhrdtech/wire_weaver_template) repository.
The examples in this repository follow the same shape: an `<name>_api` crate with the trait and types (`no_std`), and
a `<name>` crate with the host client using it (see [folder structure](../api/folder_structure.md)).

New to WireWeaver: start with `blinky_api` and `blinky` (an API with two methods and its client), then
`blinky_evolved` to see an old firmware keep working with a newer API. The rest show one feature each.

## Host side ([`examples/`](https://github.com/vhrdtech/wire_weaver/tree/master/examples))

| Example                                           | What it shows                                                               |
|---------------------------------------------------|-----------------------------------------------------------------------------|
| `minimal_shrink_wrap`                              | Using the [ShrinkWrap](../serdes/shrink_wrap.md) wire format directly         |
| `compare_wire_formats`                            | [Wire size of ShrinkWrap compared with eight other formats](wire_sizes.md)  |
| `sensor_packet`                                   | A sensor node's [reading on a radio link](../basic/types_on_the_wire.md) with ShrinkWrap alone, and the next firmware that adds a field |
| `blinky_api`, `blinky`                            | Smallest API with two methods, and its client                               |
| `thermometer_api`, `thermometer_device`, `thermometer` | A sensor with units, properties, an observed value, a stream and a sink; one server for the simulated device and the firmware |
| `bench_supply_api`, `bench_supply`                | Output channels with voltage and current limits, an output switch that refuses, and a [pytest bench](https://github.com/vhrdtech/wire_weaver/tree/master/examples/bench_supply_py) through the ww hub |
| `blinky_api_evolved`, `blinky_evolved`            | The same API [evolved](../evolution/rules.md) with a new method, used with old firmware|
| `blinky_py`                                       | Python wheel wrapping the `blinky` client                                   |
| `all_gpio_api`, `all_gpio`                        | Arrays of [traits](../api/traits.md) from the [standard library](../std_library/ww_gpio.md) |
| `uart_api`, `uart`                                | USB/Ethernet/CAN to UART bridge                                             |
| `settings_api`, `settings`                        | Two properties kept across a reboot by [`ww_settings`](../basic/settings.md), on a simulated flash |
| `defmt_log_api`, `defmt_log`                      | The firmware's `defmt` log streamed to the host with [`ww_log`](../std_library/ww_log.md) (firmware: `rp2`) |

`minimal_shrink_wrap` writes a `bool` and a `u8` by hand:

```rust
--8<-- "examples/minimal_shrink_wrap/src/main.rs:write"
```

The `bool` takes the first bit of the first byte, and the `u8` starts on the next byte:

```text
--8<-- "docs/generated/minimal_shrink_wrap/two_values.txt"
```

`compare_wire_formats` encodes eight example messages with ShrinkWrap and eight other pure-Rust formats and compares the byte counts: see [wire sizes compared](wire_sizes.md).

## In other repos

| Example | What it shows |
|---|---|
| [motor_sim](https://github.com/vhrdtech/motor_sim): `motor_demo_api`, `motor_demo` | [Motor controller demo](motor_demo.md): ~50 properties observed in three rate tiers, on a simulated motor |

## Firmware ([`examples_mcu/`](https://github.com/vhrdtech/wire_weaver/tree/master/examples_mcu))

Firmware implementing the APIs above for real boards, using `embassy`. Each board is a separate project, excluded from
the repository's Cargo workspace, and built from its own directory.

| Board                                             | MCU                                                                         |
|---------------------------------------------------|-----------------------------------------------------------------------------|
| `nucleo_g0b1re`                                   | STM32G0B1RE, Nucleo board                                                   |
| `nucleo_h743zi2`                                  | STM32H743ZI, Nucleo board                                                   |
| `usb_stm32h725ig`                                 | STM32H725IG with a USB ULPI PHY (480 Mbit/s)                                |
| `rp2`                                             | Raspberry Pi RP2350                                                         |
| `ecc_flash_g0b1`                                  | STM32G0B1, hardware test of [ECC-safe flash](../api/ecc_flash.md)            |
| `mcu_qemu`                                        | Cortex-M3 emulated in QEMU (`all_gpio_api`)                                 |

A firmware short of flash or RAM for code: linker relaxation on RISC-V saves about 10 %, and on very small
targets a byte-wise `memcpy` about 320 bytes, with downsides, see
[code size on small CPUs](../serdes/code_size.md#build-settings).

A connected device can be inspected with the [command line tool](../cli.md): `ww list`, then `ww introspect`.
