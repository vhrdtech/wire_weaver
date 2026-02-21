# blinky_api

> API for an examply blinky device.

This crate exists only for demonstration purposes. In normal use, the original API crate should be changed and its
version
bumped.

How to see it in action:

* Flash the device, firmware uses the `blinky_api` crate with version `0.1.0`.
* Run `old_firmware` example from `blink_evolved` crate.
* Observe user-friendly error, that firmware with API version `0.1.0` does not support the new method.