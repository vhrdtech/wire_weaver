//! The commands, one module each; every module registers its command with the [`crate::registry`].

pub mod api;
pub mod call;
pub(crate) mod config;
pub(crate) mod elf;
pub(crate) mod explain;
pub(crate) mod hub;
pub mod info;
pub mod introspect;
pub(crate) mod list;
pub(crate) mod observe;
pub(crate) mod settings;
pub(crate) mod stream;
pub mod sync;
#[cfg(target_os = "linux")]
pub(crate) mod udev;
pub(crate) mod usb_loopback;
