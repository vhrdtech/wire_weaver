# blinky_device

> The blinky example device: one server for the simulated device on a PC and the firmware of every board

`Blinky` holds the handlers of `blinky_api::BlinkyApi`, written once. A board gives it its LED through the `Led`
trait, and `serve` runs it on any `ww_device::Server`. The example `blinky_sim` of `examples/blinky` serves it over
WebSocket on a PC, `usb_blinky` of `examples_mcu/nucleo_g0b1re` over USB, and the tests in process.
