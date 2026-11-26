# blinky_api

> API of the blinky example device: an LED turned on and off, the first device of the book

`BlinkyApi` has four methods: `led_on()` and `led_off()`, `set_led(on: bool) -> bool` (returns whether the LED was
on before) and `blinks()` (how many times it went on). The device side is `blinky_device`, the host side `blinky`.
`blinky_api_evolved` is the same API one revision later.
