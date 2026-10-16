#![cfg_attr(not(feature = "std"), no_std)]
//! Settings store for devices (STD-32): a struct of fields with a default each, kept in NOR flash as a record of
//! the fields that differ from their default, power-cut safe, and evolvable like any WireWeaver type.
//!
//! - [`#[derive(Settings)]`](derive@Settings) on a plain struct: `#[setting(slot = N, default = ..)]` per field,
//!   `#[settings(retired(..))]` for the slots of removed fields.
//! - [Settings](struct@Settings): one group's value in RAM, [set](Settings::set) with write-on-change and a [Debounce],
//!   [persist](Settings::persist) for a write that has to be done before answering.
//! - [Store]: the records of all groups in a range of flash (an `embedded-storage-async` `NorFlash`), one [Key]
//!   per group, a small log that survives a power cut at any point.
//! - [record]: the stored format.
//!
//! See `docs/api/settings.md` for how many pages to give it, what an erase stalls on STM32G0 / F4 / H7,
//! and how fields may change between firmware versions.
//!
//! ```
//! use ww_settings::{Settings, SettingsGroup};
//!
//! #[derive(Settings, Clone, Debug, PartialEq)]
//! #[settings(retired(2))]
//! pub struct UserSettings {
//!     #[setting(slot = 0, default = 100)]
//!     pub meter_window_ms: u16,
//!     #[setting(slot = 1, default = false)]
//!     pub plug_vbus_at_boot: bool,
//!     #[setting(slot = 3, default = [0; 4])]
//!     pub led_levels: [u8; 4],
//! }
//!
//! let s = UserSettings::default();
//! assert_eq!(s.meter_window_ms, 100);
//! assert!(!s.is_set(0));
//! ```

// for the paths `#[derive(Settings)]` generates, in this crate's own tests
extern crate self as ww_settings;

pub mod group;
pub mod record;
mod settings;
pub mod store;

#[cfg(feature = "dump")]
pub mod dump;
#[cfg(feature = "fake-flash")]
pub mod fake_flash;

pub use group::{SettingsGroup, Slot, Version};
pub use settings::{Debounce, Error, LoadReport, Persisted, Settings};
pub use shrink_wrap;
pub use store::{ConfigError, Key, Store, StoreError};
pub use ww_settings_derive::Settings;
