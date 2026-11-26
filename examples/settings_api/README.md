# settings_api

> API of the settings example: two settings kept across a reboot

An API-only crate (`no_std` by default, `std`/`defmt` features), depending on `ww_api` and `ww_settings`.
Declares `SettingsDemo` (`meter_window`, `led_on_at_boot`), two user settings kept across a reboot by
`ww_settings`: written to flash only once changed and left alone for a debounce window, not on every write.
Used by the `settings` example client/server pair.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how to write an API crate.
