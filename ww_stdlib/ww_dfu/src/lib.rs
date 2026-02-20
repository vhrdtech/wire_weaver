#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_trait]
trait FirmwareUpdate {
    // const BUFFER_SIZE: u32 = _;
    fn state() -> FirmwareUpdateState;
    sink!(firmware_block: RefVec<'i, u8>); // or fn write() -> Result<(), E>?
    fn reboot();
    fn revert() -> Result<(), Error>;
    fn capabilities() -> FirmwareUpdateCapabilities;
}

pub enum FirmwareUpdateState {
    Normal,
    Reverted,
    UpdatedNotConfirmed,
}

pub struct FirmwareUpdateCapabilities {
    pub ab_partitions: bool,
    pub need_reboot_for_writing: bool,
    pub revert_supported: bool,
    /// Whether firmware can do FLASH writes and continue to execute without blocking (single bank FLASH MCUs usually cannot do that)
    pub async_write: bool,
}

pub enum Error {}
