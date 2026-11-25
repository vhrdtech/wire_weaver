# thermometer_device

The thermometer example device (`thermometer_api`): its server, handlers and event loop, written once and `no_std`.
The simulated device on a PC (`examples/thermometer`), the firmware of the Nucleo G0B1RE and the Pico 2
(`examples_mcu/nucleo_g0b1re`, `examples_mcu/rp2`) and the tests all run `thermometer_device::serve`; they differ
only in the `Board` they pass (the simulated `SimBoard`, or a chip's temperature sensor) and in the transport.
