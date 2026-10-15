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
