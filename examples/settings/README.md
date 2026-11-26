# settings

> Two settings kept across a reboot by ww_settings on a simulated flash

Depends on `settings_api`, `wire_weaver`, `wire_weaver_client`, `ww_client_server`, `ww_device`, `ww_settings`
(`fake-flash` feature) and `ww_si`. `Device` holds `meter_window` and `led_on_at_boot` as plain fields, written directly by
the generated property setters; a change notifies `Settings`, and `serve`'s periodic tick writes it to the
simulated flash once a debounce window has passed. `cargo run -p settings --example settings_reboot` connects a
client, changes both settings, waits past the debounce, stops the device and reopens it from the same flash image;
`tests/reboot.rs` runs it on tokio's paused clock and checks what it prints against the book's copy.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
