# blinky_device

> The blinky example device: one server for the simulated device on a PC and the firmware of every board

`Blinky` holds the handlers of `blinky_api::BlinkyApi`, written once. A board gives it its LED through the `Led`
trait, and `serve` runs it on any `ww_device::Server`: over WebSocket on a PC (`examples/blinky`, `blinky_sim`),
over USB on the Nucleo G0B1RE (`examples_mcu/nucleo_g0b1re`, `usb_blinky`), in process in the tests.
