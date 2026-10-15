#![no_std]

use wire_weaver::prelude::*;

/// Two user settings kept across a reboot by `ww_settings` (see `examples/settings` and
/// `docs/std_library/ww_settings.md`): written only once changed and left alone for a debounce window, not on
/// every write.
#[ww_api_root]
pub trait SettingsDemo {
    /// Averaging window of a sensor reading, milliseconds.
    property!(rw meter_window_ms: u16);
    /// Whether the user LED lights up at boot.
    property!(rw led_on_at_boot: bool);
}

/// The group kept in `ww_settings`'s [Store](ww_settings::Store), one field per property above, at the same
/// slot across every implementation (`examples/settings`, `examples_mcu/nucleo_g0b1re`,
/// `examples_mcu/nucleo_h743zi2`) so they all read and write the same record shape.
#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
pub struct UserSettings {
    #[setting(slot = 0, default = 100)]
    pub meter_window_ms: u16,
    #[setting(slot = 1, default = false)]
    pub led_on_at_boot: bool,
}
