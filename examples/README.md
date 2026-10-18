# Examples

Each example is a pair: `<name>_api` (the trait and types, `no_std`) and `<name>` (the host side using it). The
docs page [Examples and templates](../docs/examples/examples.md) lists them with what each one shows. Roughly by
increasing complexity:

* `minimal_shrink_wrap` - using the ShrinkWrap wire format directly
* `compare_wire_formats` - wire size of ShrinkWrap compared with other formats
* `blinky_api`, `blinky` - the smallest API with two methods, and its client; start here
* `blinky_api_evolved`, `blinky_evolved` - the same API evolved with a new method, used with old firmware
* `blinky_py` - Python wheel wrapping the `blinky` client
* `all_gpio_api`, `all_gpio` - arrays of traits from the standard library (`ww_gpio`)
* `uart_api`, `uart` - USB/Ethernet/CAN to UART bridge
* `defmt_log_api`, `defmt_log` - the firmware's `defmt` log streamed to the host with `ww_log`
* `settings_api`, `settings` - two properties kept across a reboot by `ww_settings`, on a simulated flash

The motor controller demo (`motor_demo_api`, `motor_demo`: a VESC-like FOC controller on a simulated motor, its ~50
variables observed in three rate tiers) lives in [motor_sim](https://github.com/vhrdtech/motor_sim); see `docs/examples/motor_demo.md`.
