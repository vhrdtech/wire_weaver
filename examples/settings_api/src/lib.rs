//! API of the settings example: two properties kept across a reboot by `ww_settings`, see `examples/settings`.
#![no_std]

use ww_api::prelude::*;
use ww_si::prefix::Milli;
use ww_si::units::Second;
use ww_si::{Fixed, Range};

/// Averaging window of a sensor reading: 10 ms to 10 s in steps of 1 ms.
pub type MeterWindow = Fixed<Second, Milli, u16, Range<10, 10_000>>;

// --8<-- [start:api]
/// Two user settings kept across a reboot by `ww_settings` (see `examples/settings` and the book's chapter
/// "Settings that survive a reboot"): written once changed and left alone for a debounce window, not on every write.
#[ww_api_root]
pub trait SettingsDemo {
    /// Averaging window of a sensor reading.
    #[effect(edit, persist)]
    property!(rw meter_window: MeterWindow);
    /// Whether the user LED lights up at boot.
    #[effect(edit, persist)]
    property!(rw led_on_at_boot: bool);
}
// --8<-- [end:api]

// --8<-- [start:settings]
/// The group kept in `ww_settings`'s [Store](ww_settings::Store), one field per property above, at the same
/// slot across every implementation (`examples/settings`, `examples_mcu/nucleo_g0b1re`,
/// `examples_mcu/nucleo_h743zi2`) so they all read and write the same record shape.
#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
pub struct UserSettings {
    #[setting(slot = 0, default = MeterWindow::bounded(100))]
    pub meter_window: MeterWindow,
    #[setting(slot = 1, default = false)]
    pub led_on_at_boot: bool,
}
// --8<-- [end:settings]
