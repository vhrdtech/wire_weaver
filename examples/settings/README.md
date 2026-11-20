# settings

> Two settings kept across a reboot by ww_settings on a simulated flash

Depends on `settings_api`, `wire_weaver`, `wire_weaver_client`, `ww_client_server`, `ww_device` and `ww_settings`
(`fake-flash` feature). `Device` holds `meter_window_ms` and `led_on_at_boot` as plain fields, written directly by
the generated property setters; a change notifies `Settings`, and `serve`'s periodic tick writes it to the
simulated flash once a debounce window has passed. `tests/reboot.rs` connects a client, changes both settings,
waits past the debounce, stops the device and reopens it from the same flash image to check they survived.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
