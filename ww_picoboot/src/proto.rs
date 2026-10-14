//! PICOBOOT wire format: commands, status codes and constants, as in pico-sdk `boot/picoboot.h` and
//! `boot/picoboot_constants.h`. Sans-IO, shared by the host ([crate::Picoboot]) and the simulated device.

use std::time::Duration;

/// Raspberry Pi USB vendor id.
pub const RPI_VID: u16 = 0x2e8a;
/// RP2040 boot ROM in BOOTSEL mode.
pub const RP2040_BOOTSEL_PID: u16 = 0x0003;
/// RP2350 boot ROM in BOOTSEL mode.
pub const RP2350_BOOTSEL_PID: u16 = 0x000f;

/// The PICOBOOT interface: vendor specific, two bulk endpoints (the other interface is the UF2 drive, class 8).
pub const INTERFACE_CLASS: u8 = 0xff;

pub const MAGIC: u32 = 0x431f_d10b;
pub const CMD_LEN: usize = 32;

/// Interface control request (vendor, host to device): abort the command in progress, clear the stalls.
pub const IF_RESET: u8 = 0x41;
/// Interface control request (vendor, device to host): the 16-byte [CmdStatus] of the last command.
pub const IF_CMD_STATUS: u8 = 0x42;

/// Start of flash in the address space (XIP).
pub const FLASH_BASE: u32 = 0x1000_0000;
/// Erase unit, `FLASH_ERASE` address and size are multiples of it.
pub const FLASH_SECTOR_SIZE: usize = 4096;
/// Program unit, flash `WRITE` address and size are multiples of it.
pub const FLASH_PAGE_SIZE: usize = 256;

/// The delay before the reboot after `REBOOT2`, long enough for the reply to reach the host.
pub const REBOOT_DELAY: Duration = Duration::from_millis(500);

/// Longest `GET_INFO` reply asked for.
pub const GET_INFO_MAX_LEN: u32 = 256;
/// `GET_INFO` type: system information (`rom_get_sys_info`). The reply: word count, the flags included, then the
/// words of each included item in flag order.
pub const GET_INFO_SYS: u8 = 1;
/// `GET_INFO` SYS flag: chip info, 3 words: package id, chip id low word, chip id high word.
pub const SYS_INFO_CHIP_INFO: u32 = 0x0001;

/// Command ids, the top bit set for commands whose data goes device to host.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CmdId {
    ExclusiveAccess = 0x01,
    /// RP2040 only, RP2350 has [CmdId::Reboot2].
    Reboot = 0x02,
    FlashErase = 0x03,
    Read = 0x84,
    Write = 0x05,
    ExitXip = 0x06,
    EnterCmdXip = 0x07,
    Exec = 0x08,
    VectorizeFlash = 0x09,
    /// RP2350.
    Reboot2 = 0x0a,
    /// RP2350.
    GetInfo = 0x8b,
    /// RP2350.
    OtpRead = 0x8c,
    /// RP2350.
    OtpWrite = 0x0d,
}

impl CmdId {
    pub fn from_u8(id: u8) -> Option<CmdId> {
        use CmdId::*;
        [
            ExclusiveAccess,
            Reboot,
            FlashErase,
            Read,
            Write,
            ExitXip,
            EnterCmdXip,
            Exec,
            VectorizeFlash,
            Reboot2,
            GetInfo,
            OtpRead,
            OtpWrite,
        ]
        .into_iter()
        .find(|c| *c as u8 == id)
    }

    /// Data (if any) goes device to host, the status phase is a zero-length packet host to device.
    pub fn is_in(self) -> bool {
        self as u8 & 0x80 != 0
    }

    pub fn name(self) -> &'static str {
        match self {
            CmdId::ExclusiveAccess => "EXCLUSIVE_ACCESS",
            CmdId::Reboot => "REBOOT",
            CmdId::FlashErase => "FLASH_ERASE",
            CmdId::Read => "READ",
            CmdId::Write => "WRITE",
            CmdId::ExitXip => "EXIT_XIP",
            CmdId::EnterCmdXip => "ENTER_CMD_XIP",
            CmdId::Exec => "EXEC",
            CmdId::VectorizeFlash => "VECTORIZE_FLASH",
            CmdId::Reboot2 => "REBOOT2",
            CmdId::GetInfo => "GET_INFO",
            CmdId::OtpRead => "OTP_READ",
            CmdId::OtpWrite => "OTP_WRITE",
        }
    }
}

/// `struct picoboot_cmd` without magic and token: id, argument bytes, data phase length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub id: CmdId,
    pub args: [u8; 16],
    pub args_len: u8,
    pub transfer_len: u32,
}

impl Cmd {
    pub fn new(id: CmdId, args: &[u8], transfer_len: u32) -> Cmd {
        let mut a = [0u8; 16];
        a[..args.len()].copy_from_slice(args);
        Cmd {
            id,
            args: a,
            args_len: args.len() as u8,
            transfer_len,
        }
    }

    /// `struct picoboot_range_cmd`: address and size.
    pub fn range(id: CmdId, addr: u32, size: u32, transfer_len: u32) -> Cmd {
        let mut args = [0u8; 8];
        args[..4].copy_from_slice(&addr.to_le_bytes());
        args[4..].copy_from_slice(&size.to_le_bytes());
        Cmd::new(id, &args, transfer_len)
    }

    pub fn encode(&self, token: u32) -> [u8; CMD_LEN] {
        let mut b = [0u8; CMD_LEN];
        b[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        b[4..8].copy_from_slice(&token.to_le_bytes());
        b[8] = self.id as u8;
        b[9] = self.args_len;
        b[12..16].copy_from_slice(&self.transfer_len.to_le_bytes());
        b[16..32].copy_from_slice(&self.args);
        b
    }

    /// Decode a command packet: token and command, or the status a device answers a bad one with.
    pub fn decode(b: &[u8]) -> Result<(u32, Cmd), Status> {
        if b.len() != CMD_LEN || u32::from_le_bytes(b[0..4].try_into().unwrap()) != MAGIC {
            return Err(Status::InvalidCmdLength);
        }
        let token = u32::from_le_bytes(b[4..8].try_into().unwrap());
        let id = CmdId::from_u8(b[8]).ok_or(Status::UnknownCmd)?;
        let args_len = b[9];
        if args_len > 16 {
            return Err(Status::InvalidCmdLength);
        }
        Ok((
            token,
            Cmd {
                id,
                args: b[16..32].try_into().unwrap(),
                args_len,
                transfer_len: u32::from_le_bytes(b[12..16].try_into().unwrap()),
            },
        ))
    }

    /// Argument word `i` (little endian).
    pub fn arg_u32(&self, i: usize) -> u32 {
        u32::from_le_bytes(self.args[i * 4..i * 4 + 4].try_into().unwrap())
    }
}

/// `REBOOT2` reboot types (RP2350).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Reboot2 {
    /// Boot normally: the image in flash.
    Normal,
    /// Boot the image just written at `base` (a flash address): what `picotool load -x` sends after writing flash.
    /// The boot ROM prefers the updated partition and starts a try-before-you-buy image in its trial.
    FlashUpdate { base: u32 },
    /// Back into BOOTSEL.
    Bootsel,
}

/// `REBOOT2_FLAG_REBOOT_TYPE_NORMAL`
pub const REBOOT2_TYPE_NORMAL: u32 = 0x0;
/// `REBOOT2_FLAG_REBOOT_TYPE_BOOTSEL`
pub const REBOOT2_TYPE_BOOTSEL: u32 = 0x2;
/// `REBOOT2_FLAG_REBOOT_TYPE_FLASH_UPDATE`, `dParam0` is the flash address of the update.
pub const REBOOT2_TYPE_FLASH_UPDATE: u32 = 0x4;
/// `REBOOT2_FLAG_REBOOT_TYPE_*` mask.
pub const REBOOT2_TYPE_MASK: u32 = 0xf;

impl Reboot2 {
    pub fn cmd(self, delay: Duration) -> Cmd {
        let (flags, param0) = match self {
            Reboot2::Normal => (REBOOT2_TYPE_NORMAL, 0),
            Reboot2::FlashUpdate { base } => (REBOOT2_TYPE_FLASH_UPDATE, base),
            Reboot2::Bootsel => (REBOOT2_TYPE_BOOTSEL, 0),
        };
        let mut args = [0u8; 16];
        args[0..4].copy_from_slice(&flags.to_le_bytes());
        args[4..8].copy_from_slice(&(delay.as_millis() as u32).to_le_bytes());
        args[8..12].copy_from_slice(&param0.to_le_bytes());
        Cmd::new(CmdId::Reboot2, &args, 0)
    }
}

/// `GET_INFO` for the system info `flags`.
pub fn get_sys_info(flags: u32, max_len: u32) -> Cmd {
    let mut args = [0u8; 16];
    args[0] = GET_INFO_SYS;
    args[4..8].copy_from_slice(&flags.to_le_bytes());
    Cmd::new(CmdId::GetInfo, &args, max_len)
}

/// The chip id from the words of a `GET_INFO` SYS reply for [SYS_INFO_CHIP_INFO] (count word removed): flags
/// supported, package selection, chip id low word, chip id high word.
pub fn chip_id_from_sys_info(words: &[u32]) -> Option<u64> {
    if words.len() < 4 || words[0] & SYS_INFO_CHIP_INFO == 0 {
        return None;
    }
    Some(words[2] as u64 | (words[3] as u64) << 32)
}

/// `struct picoboot_cmd_status`, the reply to [IF_CMD_STATUS].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CmdStatus {
    pub token: u32,
    pub status: Status,
    pub cmd_id: u8,
    pub in_progress: bool,
}

impl CmdStatus {
    pub fn decode(b: &[u8; 16]) -> CmdStatus {
        CmdStatus {
            token: u32::from_le_bytes(b[0..4].try_into().unwrap()),
            status: Status::from_u32(u32::from_le_bytes(b[4..8].try_into().unwrap())),
            cmd_id: b[8],
            in_progress: b[9] != 0,
        }
    }

    pub fn encode(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..4].copy_from_slice(&self.token.to_le_bytes());
        b[4..8].copy_from_slice(&(self.status as u32).to_le_bytes());
        b[8] = self.cmd_id;
        b[9] = self.in_progress as u8;
        b
    }
}

/// `enum picoboot_status`
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[repr(u32)]
pub enum Status {
    #[error("ok")]
    Ok = 0,
    #[error("unknown command")]
    UnknownCmd = 1,
    #[error("invalid command length")]
    InvalidCmdLength = 2,
    #[error("invalid transfer length")]
    InvalidTransferLength = 3,
    #[error("invalid address")]
    InvalidAddress = 4,
    #[error("bad alignment")]
    BadAlignment = 5,
    #[error("interleaved write")]
    InterleavedWrite = 6,
    #[error("rebooting")]
    Rebooting = 7,
    #[error("unknown error")]
    UnknownError = 8,
    #[error("invalid state")]
    InvalidState = 9,
    #[error("not permitted")]
    NotPermitted = 10,
    #[error("invalid argument")]
    InvalidArg = 11,
    #[error("buffer too small")]
    BufferTooSmall = 12,
    #[error("precondition not met")]
    PreconditionNotMet = 13,
    #[error("modified data")]
    ModifiedData = 14,
    #[error("invalid data")]
    InvalidData = 15,
    #[error("not found")]
    NotFound = 16,
    #[error("unsupported modification")]
    UnsupportedModification = 17,
}

impl Status {
    pub fn from_u32(v: u32) -> Status {
        use Status::*;
        [
            Ok,
            UnknownCmd,
            InvalidCmdLength,
            InvalidTransferLength,
            InvalidAddress,
            BadAlignment,
            InterleavedWrite,
            Rebooting,
            UnknownError,
            InvalidState,
            NotPermitted,
            InvalidArg,
            BufferTooSmall,
            PreconditionNotMet,
            ModifiedData,
            InvalidData,
            NotFound,
            UnsupportedModification,
        ]
        .into_iter()
        .find(|s| *s as u32 == v)
        .unwrap_or(UnknownError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_round_trip() {
        let cmd = Cmd::range(CmdId::Write, FLASH_BASE + 256, 256, 256);
        let b = cmd.encode(7);
        assert_eq!(&b[..4], &[0x0b, 0xd1, 0x1f, 0x43]);
        assert_eq!(b[8], 0x05);
        assert_eq!(b[9], 8);
        assert_eq!(Cmd::decode(&b).unwrap(), (7, cmd));
    }
}
