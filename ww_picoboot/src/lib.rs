//! Host side of PICOBOOT, the USB interface of the Raspberry Pi RP2350 (and RP2040) boot ROM in BOOTSEL mode, in Rust
//! on [nusb](https://docs.rs/nusb): read, erase and write flash, query the chip, reboot. What `picotool` does, as a
//! library, so a host can update a device it rebooted into BOOTSEL without a button press or a separate tool.
//!
//! - [Picoboot] runs commands over any [PicobootIo]: [usb::NusbIo] on a real device, [sim::SimRp2350] (feature
//!   `sim`) in tests.
//! - [Image] holds what to write, from a UF2 file or a raw binary; [Picoboot::flash] erases, writes and verifies it.
//! - [usb::list] finds BOOTSEL devices, [usb::open] opens one by its USB serial number: the chip id, the same that a
//!   firmware puts into its own serial number so the host finds the unit again after the reboot (see
//!   `wire_weaver_client::follow`).

pub mod image;
pub mod io;
pub mod proto;
#[cfg(feature = "sim")]
pub mod sim;
#[cfg(feature = "usb")]
pub mod usb;

mod client;

pub use client::{Picoboot, Progress, Stage};
pub use image::Image;
pub use io::{IoError, PicobootIo};
pub use proto::Status;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USB: {0}")]
    Io(#[from] IoError),
    /// The device refused a command.
    #[error("{cmd} failed: {status}")]
    Command { cmd: &'static str, status: Status },
    #[error("image: {0}")]
    Image(String),
    #[error("verify failed at {addr:#010x}: flash differs from the image")]
    Verify { addr: u32 },
    #[error("{0}")]
    Other(String),
}
