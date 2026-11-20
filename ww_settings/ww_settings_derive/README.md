# ww_settings_derive

> #[derive(Settings)] for ww_settings groups

The proc-macro crate behind `ww_settings`: `#[derive(Settings)]` implements `Default` (every field at its
`default`) and `ww_settings::SettingsGroup` for a struct whose every field has
`#[setting(slot = N, default = <expr>)]`. Taken through `ww_settings`, never used on its own.

```rust,ignore
#[derive(Settings, Clone, Debug, PartialEq)]
#[settings(retired(2))] // slots of removed fields, never used again
pub struct UserSettings {
    #[setting(slot = 0, default = 100)]
    pub meter_window_ms: u16,
    #[setting(slot = 1, default = false)]
    pub plug_vbus_at_boot: bool,
}
```

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture.
