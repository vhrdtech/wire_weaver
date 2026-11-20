# ww_settings

> Power-cut safe settings store for devices: a struct of fields with defaults, kept as an evolvable
> shrink_wrap record in NOR flash

A struct of fields with a default each (STD-32), kept in NOR flash as a record of the fields that differ from
their default, power-cut safe, and evolvable like any WireWeaver type. `#[derive(Settings)]` (from
`ww_settings_derive`) on a plain struct, with `#[setting(slot = N, default = ..)]` per field and
`#[settings(retired(..))]` for removed slots; `Settings` holds one group's value in RAM with write-on-change and
a debounce, `persist` for a write that must happen before answering; `Store` keeps every group's records in a
range of flash (an `embedded-storage-async` `NorFlash`), one key per group, in a small log that survives a power
cut at any point. Depends on `shrink_wrap`, `ww_crc` and `ww_settings_derive`.

```rust
use ww_settings::{Settings, SettingsGroup};

#[derive(Settings, Clone, Debug, PartialEq)]
#[settings(retired(2))]
pub struct UserSettings {
    #[setting(slot = 0, default = 100)]
    pub meter_window_ms: u16,
    #[setting(slot = 1, default = false)]
    pub plug_vbus_at_boot: bool,
}
```

See `docs/api/settings.md` in the [main repo](https://github.com/vhrdtech/wire_weaver) for how many pages to
give it, what an erase stalls on STM32G0/F4/H7, and how fields may change between firmware versions.
