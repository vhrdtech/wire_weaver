//! Host for the STM32 system bootloader's USB DFU (DfuSe, AN3156): list devices (`0483:DF11`), read the memory layout
//! of an alternate setting, erase sectors, write with read-back verify, and leave DFU by jumping to flash.
//!
//! [Dfuse] is the protocol on top of [DfuIo], implemented for real devices on nusb ([usb], feature `usb`) and for
//! tests by [sim::SimStm32Dfu] (feature `sim`).
//!
//! The existing `dfu-core` crate was evaluated first: it owns the download loop (erase, set address, data) and has no
//! upload, so no read-back verify, no sans-IO boundary to put a simulated device behind, and no control over the
//! erase timeouts, which on an H7 take seconds per 128 KB sector.

mod client;
pub mod image;
mod io;
pub mod layout;
pub mod proto;
#[cfg(feature = "sim")]
pub mod sim;
#[cfg(feature = "usb")]
pub mod usb;

pub use client::{Dfuse, FlashOptions, Timeouts};
pub use image::Image;
pub use io::{DfuIo, IoError};
pub use layout::MemoryLayout;

use proto::DfuStatus;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USB: {0}")]
    Io(#[from] IoError),
    #[error("{what}: the device reports: {status}")]
    Device {
        what: &'static str,
        status: DfuStatus,
    },
    #[error("{0}: no answer in time")]
    Timeout(&'static str),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("memory layout {0}")]
    Layout(String),
    #[error("image: {0}")]
    Image(String),
    #[error("{len} bytes at {addr:#010x} are not in writable memory of \"{layout}\"")]
    OutOfMemory {
        addr: u32,
        len: usize,
        layout: String,
    },
    #[error("verify failed at {addr:#010x}: wrote {wrote:#04x}, read {read:#04x}")]
    Verify { addr: u32, wrote: u8, read: u8 },
    #[error("{0}")]
    Other(String),
}

/// Parts of a flash run.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Erase,
    Write,
    Verify,
}

/// Progress of a flash run: `done` of `total` bytes of the stage.
pub trait Progress {
    fn update(&mut self, stage: Stage, done: u64, total: u64);
}

impl Progress for () {
    fn update(&mut self, _: Stage, _: u64, _: u64) {}
}

impl<F: FnMut(Stage, u64, u64)> Progress for F {
    fn update(&mut self, stage: Stage, done: u64, total: u64) {
        self(stage, done, total)
    }
}

/// The serial number string the STM32 system bootloader reports for a chip with unique id words `uid`
/// (`UID[31:0]`, `UID[63:32]`, `UID[95:64]`), as ST's USB device library and the ROM form it: the sum of the first
/// and third word as 8 hex digits, then the upper 16 bits of the second word as 4 hex digits.
///
/// NOT yet checked against a real H72x ROM (AN3156 doesn't spell it out): compare with `ww stm32dfu list` on
/// hardware, and pass the serial from the list when they differ.
pub fn rom_serial(uid: [u32; 3]) -> String {
    format!("{:08X}{:04X}", uid[0].wrapping_add(uid[2]), uid[1] >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_from_uid() {
        assert_eq!(rom_serial([0x0000_0010, 0x1234_5678, 0x0000_0001]), "000000111234");
    }
}
