//! DFU 1.1 and the STMicroelectronics DfuSe extension (AN3156), as the system bootloader speaks them.

/// STMicroelectronics.
pub const ST_VID: u16 = 0x0483;
/// The system bootloader in DFU mode (AN2606: every STM32 with USB DFU).
pub const ROM_DFU_PID: u16 = 0xDF11;
/// Interface class / subclass of DFU (application specific / DFU).
pub const DFU_CLASS: u8 = 0xFE;
pub const DFU_SUBCLASS: u8 = 0x01;
/// Descriptor type of the DFU functional descriptor.
pub const DFU_FUNCTIONAL_DESCRIPTOR: u8 = 0x21;
/// Transfer size when the functional descriptor can't be read (the H7 bootloader reports 1024).
pub const DEFAULT_TRANSFER_SIZE: usize = 1024;

pub const DFU_DNLOAD: u8 = 1;
pub const DFU_UPLOAD: u8 = 2;
pub const DFU_GETSTATUS: u8 = 3;
pub const DFU_CLRSTATUS: u8 = 4;
pub const DFU_GETSTATE: u8 = 5;
pub const DFU_ABORT: u8 = 6;

/// DfuSe commands, sent as the data of a `DFU_DNLOAD` with `wValue = 0`.
pub const CMD_GET_COMMANDS: u8 = 0x00;
pub const CMD_SET_ADDRESS: u8 = 0x21;
pub const CMD_ERASE: u8 = 0x41;
pub const CMD_READ_UNPROTECT: u8 = 0x92;
/// `wValue` of the first data block: block `n` is at `address + (n - 2) * transfer_size`.
pub const FIRST_DATA_BLOCK: u16 = 2;

/// `bState`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    AppIdle,
    AppDetach,
    DfuIdle,
    DnloadSync,
    DnBusy,
    DnloadIdle,
    ManifestSync,
    Manifest,
    ManifestWaitReset,
    UploadIdle,
    Error,
    Other(u8),
}

impl From<u8> for State {
    fn from(v: u8) -> Self {
        match v {
            0 => State::AppIdle,
            1 => State::AppDetach,
            2 => State::DfuIdle,
            3 => State::DnloadSync,
            4 => State::DnBusy,
            5 => State::DnloadIdle,
            6 => State::ManifestSync,
            7 => State::Manifest,
            8 => State::ManifestWaitReset,
            9 => State::UploadIdle,
            10 => State::Error,
            v => State::Other(v),
        }
    }
}

impl From<State> for u8 {
    fn from(s: State) -> u8 {
        match s {
            State::AppIdle => 0,
            State::AppDetach => 1,
            State::DfuIdle => 2,
            State::DnloadSync => 3,
            State::DnBusy => 4,
            State::DnloadIdle => 5,
            State::ManifestSync => 6,
            State::Manifest => 7,
            State::ManifestWaitReset => 8,
            State::UploadIdle => 9,
            State::Error => 10,
            State::Other(v) => v,
        }
    }
}

/// `bStatus`: 0 is OK, the rest are errors (DFU 1.1 table 4.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DfuStatus {
    #[error("OK")]
    Ok,
    #[error("target: file is not for this device")]
    Target,
    #[error("file failed vendor verification")]
    File,
    #[error("unable to write memory (write protected, or the address is not writable)")]
    Write,
    #[error("memory erase failed (erase protected, or no sector there)")]
    Erase,
    #[error("memory erase check failed")]
    CheckErased,
    #[error("program memory failed")]
    Prog,
    #[error("programmed memory failed verification")]
    Verify,
    #[error("received address is out of range")]
    Address,
    #[error("received DFU_DNLOAD with wLength = 0 but the device does not think it has all the data")]
    NotDone,
    #[error("firmware is corrupt")]
    Firmware,
    #[error("vendor specific error")]
    Vendor,
    #[error("unexpected USB reset")]
    UsbReset,
    #[error("unexpected power on reset")]
    PowerOnReset,
    #[error("unknown error")]
    Unknown,
    #[error("unexpected request (stalled)")]
    StalledPacket,
    #[error("status {0:#x}")]
    Other(u8),
}

impl From<u8> for DfuStatus {
    fn from(v: u8) -> Self {
        match v {
            0x00 => DfuStatus::Ok,
            0x01 => DfuStatus::Target,
            0x02 => DfuStatus::File,
            0x03 => DfuStatus::Write,
            0x04 => DfuStatus::Erase,
            0x05 => DfuStatus::CheckErased,
            0x06 => DfuStatus::Prog,
            0x07 => DfuStatus::Verify,
            0x08 => DfuStatus::Address,
            0x09 => DfuStatus::NotDone,
            0x0A => DfuStatus::Firmware,
            0x0B => DfuStatus::Vendor,
            0x0C => DfuStatus::UsbReset,
            0x0D => DfuStatus::PowerOnReset,
            0x0E => DfuStatus::Unknown,
            0x0F => DfuStatus::StalledPacket,
            v => DfuStatus::Other(v),
        }
    }
}

impl From<DfuStatus> for u8 {
    fn from(s: DfuStatus) -> u8 {
        match s {
            DfuStatus::Ok => 0x00,
            DfuStatus::Target => 0x01,
            DfuStatus::File => 0x02,
            DfuStatus::Write => 0x03,
            DfuStatus::Erase => 0x04,
            DfuStatus::CheckErased => 0x05,
            DfuStatus::Prog => 0x06,
            DfuStatus::Verify => 0x07,
            DfuStatus::Address => 0x08,
            DfuStatus::NotDone => 0x09,
            DfuStatus::Firmware => 0x0A,
            DfuStatus::Vendor => 0x0B,
            DfuStatus::UsbReset => 0x0C,
            DfuStatus::PowerOnReset => 0x0D,
            DfuStatus::Unknown => 0x0E,
            DfuStatus::StalledPacket => 0x0F,
            DfuStatus::Other(v) => v,
        }
    }
}

/// The 6 bytes of `DFU_GETSTATUS`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub status: DfuStatus,
    /// Minimum time to wait before the next `DFU_GETSTATUS`, in milliseconds (`bwPollTimeout`).
    pub poll_timeout_ms: u32,
    pub state: State,
}

impl Status {
    pub const LEN: usize = 6;

    pub fn parse(b: &[u8]) -> Option<Status> {
        if b.len() < Self::LEN {
            return None;
        }
        Some(Status {
            status: b[0].into(),
            poll_timeout_ms: u32::from_le_bytes([b[1], b[2], b[3], 0]),
            state: b[4].into(),
        })
    }

    pub fn to_bytes(self) -> [u8; Self::LEN] {
        let t = self.poll_timeout_ms.to_le_bytes();
        [self.status.into(), t[0], t[1], t[2], self.state.into(), 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trip() {
        let s = Status {
            status: DfuStatus::Erase,
            poll_timeout_ms: 0x01_02_03,
            state: State::DnBusy,
        };
        assert_eq!(Status::parse(&s.to_bytes()), Some(s));
        assert_eq!(Status::parse(&[0; 5]), None);
    }
}
