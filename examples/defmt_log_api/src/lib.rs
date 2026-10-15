//! API of the defmt log example: the firmware's defmt log as `ww_log::LogDefmt`, and a way to overload the logger
//! on purpose. Firmware: `examples_mcu/rp2` (`rp2_usb_log`), host: `examples/defmt_log`.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait DefmtLogDemo {
    ww_impl!(log: ww_log::LogDefmt);

    /// Logs `count` frames in a row without pumping in between: what doesn't fit the logger's ring reaches the host
    /// as exactly that many lost log messages.
    fn flood(count: u32);
}
