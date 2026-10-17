#![cfg_attr(not(feature = "std"), no_std)]
//! Raw access to an SD card over WireWeaver (STD-40): its state, its registers as the card sent them, the init
//! sequence with what each step answered, and single raw commands with their response, status bits and timing.
//!
//! [SdCard] is for bring-up, card checks and odd cards; reading and writing data is `ww_block`'s
//! `BlockDevice`, served for the same card. On the device it is a passthrough to the SDMMC (or SPI) driver.
//!
//! Raw commands are sorted by what they can do ([access], [Access]):
//!
//! - **Free**: reads and status queries (CMD9, CMD10, CMD13, CMD17/18, CMD6 in check mode, ACMD13, ACMD22,
//!   ACMD51, ...) work at any time.
//! - **Lease**: everything else changes the card's data or state (writes, CMD38 erase, CMD6 switch, CMD0 reset,
//!   bus width, ...) and needs the lease of `ww_block::BlockDevice::claim` for the same card, which stops the
//!   card's own user first. [SdCard::reinit] needs it too.
//! - **Restricted**: commands that can lock a card or damage it for good, CMD42 (lock, unlock, passwords, force
//!   erase) and CMD27 (program CSD, whose permanent write-protect bit can't be cleared), are refused with
//!   [SdError::Restricted], also with a lease. A firmware built with this crate's feature `raw-sd-unrestricted`
//!   lets them through (with a lease): for bring-up builds only. [Capabilities::unrestricted] tells a host.
//!
//! [check_access] is that rule for a device's handler. Who may call the lease commands at all is the firmware's
//! choice per link, made where it hands out the lease (`ww_block` answers `Common(Refused)`); this crate has no
//! access rules of its own.
//!
//! Serve it with `ww api scaffold`, which prints the handler stubs for the `ww_codegen!` call you use.

use wire_weaver::prelude::*;
use ww_error::CommonError;

/// Most bytes of a raw command's data phase, either direction: 8 blocks of 512 bytes.
pub const MAX_DATA: usize = 4096;

/// Most steps an [InitReport] lists.
pub const MAX_INIT_STEPS: usize = 24;

/// Raw access to an SD card: registers, init, single commands.
///
/// # Errors
///
/// [SdError]: the `Common` variants and the card's own, listed on each call.
#[ww_trait]
pub trait SdCard {
    /// What the device's host controller can do: interface, bus widths, clock, UHS, the size of a data phase,
    /// whether restricted commands go through.
    fn capabilities() -> Capabilities;

    /// The card in the slot and how it is driven now.
    fn state() -> CardState;

    /// Card inserted (`true`) or removed (`false`), from the card detect switch or a failing poll.
    stream!(detect: bool);

    /// The card's registers as raw bytes, as the card sent them (MSB first): CID, CSD, SCR, OCR, SD Status
    /// (ACMD13), and the RCA. Decoding is the host's. Read at init and cached, safe at any time.
    ///
    /// `NoCard`, `Common(WrongState)` before the card is initialised.
    #[errors(Common(WrongState), NoCard)]
    fn registers() -> Result<SdRegisters<'i>, SdError<'i>>;

    /// Runs the init sequence again (CMD0, CMD8, ACMD41, CMD2, CMD3, ..., then bus width and speed up to the
    /// limits given) and reports what each step answered and how long it took. Needs the lease (see the crate
    /// docs): the card's user loses the card meanwhile.
    ///
    /// A card that fails to come up is not an error: the report says where it stopped. `Common(Unsupported)` for
    /// a bus width or clock the controller can't do.
    #[errors(Common(Busy, Unsupported), NoCard, BadLease)]
    fn reinit(
        lease: u32,
        max_clock_khz: u32,
        bus_width: BusWidth,
    ) -> Result<InitReport<'i>, SdError<'i>>;

    /// One raw command, with an optional data phase, see [RawCommand]. Free commands need no lease, the others
    /// the current one (see the crate docs).
    ///
    /// What the card did (answered, timed out, CRC error) is in [RawResponse::outcome], with whatever bytes and
    /// timing there were; an error here means the command wasn't sent. `Restricted` for CMD42 and CMD27 without
    /// `raw-sd-unrestricted`, `Common(Unsupported)` for a command the interface doesn't have (SPI mode),
    /// `Common(InvalidArgument)` for an index above 63 or a data phase over [MAX_DATA].
    #[errors(
        Common(Busy, Unsupported, InvalidArgument),
        NoCard,
        BadLease,
        Restricted
    )]
    fn command(lease: Option<u32>, cmd: RawCommand<'i>) -> Result<RawResponse<'i>, SdError<'i>>;
}

/// What the host controller can do, see [SdCard::capabilities].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Capabilities {
    pub interface: Interface,
    /// The widest bus.
    pub max_bus_width: BusWidth,
    /// Highest card clock in kHz.
    pub max_clock_khz: u32,
    /// 1.8 V signalling and the UHS-I modes.
    pub uhs: bool,
    /// Built with `raw-sd-unrestricted`: CMD42 and CMD27 go through (with a lease).
    pub unrestricted: bool,
}

/// How the controller talks to the card, see [Capabilities::interface].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Interface {
    /// The SD bus (an SDMMC/SDIO peripheral).
    Sd,
    /// SPI mode: some commands don't exist, responses are R1-based.
    Spi,
}

/// Width of the data bus, see [CardState::bus] and [SdCard::reinit].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum BusWidth {
    One,
    Four,
    /// eMMC only.
    Eight,
}

/// The card and how it is driven, see [SdCard::state].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct CardState {
    /// A card is in the slot (detect switch, or it answered).
    pub present: bool,
    /// The card's capacity class, `None` before init.
    pub kind: Option<CardKind>,
    /// The bus as set up by the last init, `None` while the card is not initialised.
    pub bus: Option<BusState>,
    /// A lease of `ww_block` is held: the card's user is stopped.
    pub leased: bool,
}

/// Capacity class of a card (OCR `CCS`, CSD version, size), see [CardState::kind].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum CardKind {
    /// Up to 2 GB, byte addressed.
    Sdsc,
    /// Up to 32 GB.
    Sdhc,
    /// Up to 2 TB.
    Sdxc,
    /// Up to 128 TB.
    Sduc,
}

/// The bus after init, see [CardState::bus].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct BusState {
    pub width: BusWidth,
    /// Card clock in kHz as set (the nearest the divider gives).
    pub clock_khz: u32,
    pub speed: SpeedMode,
    /// Signalling at 1.8 V (UHS-I) instead of 3.3 V.
    pub low_voltage: bool,
}

/// Bus speed mode (CMD6 function group 1), see [BusState::speed].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum SpeedMode {
    /// Up to 25 MHz.
    DefaultSpeed,
    /// Up to 50 MHz.
    HighSpeed,
    Sdr12,
    Sdr25,
    Sdr50,
    Sdr104,
    Ddr50,
}

/// The card's registers, raw, see [SdCard::registers]. All MSB first, as the card sends them; the CRC7 byte at the
/// end of CID and CSD is as the controller delivered it (often zero).
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct SdRegisters<'i> {
    /// Card identification (CMD2/CMD10).
    pub cid: [u8; 16],
    /// Card specific data (CMD9).
    pub csd: [u8; 16],
    /// Operating conditions (ACMD41's answer).
    pub ocr: u32,
    /// Relative card address (CMD3), 0 in SPI mode.
    pub rca: u16,
    /// SD configuration (ACMD51), `None` if it couldn't be read.
    pub scr: Option<[u8; 8]>,
    /// SD Status (ACMD13, 64 bytes), `None` if it couldn't be read.
    pub ssr: Option<MaxLen<RefVec<'i, u8>, 64>>,
}

/// What the init sequence did, see [SdCard::reinit].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct InitReport<'i> {
    /// The steps in order, up to [MAX_INIT_STEPS]; repeats of a command (ACMD41 while the card is busy) are one step.
    pub steps: MaxLen<RefVec<'i, InitStep>, MAX_INIT_STEPS>,
    /// The state it ended in; `bus` is `None` when the card didn't come up.
    pub state: CardState,
    /// Microseconds for the whole sequence.
    pub total_us: u32,
}

/// One step of an [InitReport]: a command and what the card answered last.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct InitStep {
    /// Command index; an application command (CMD55 first) when `app`.
    pub index: u8,
    pub app: bool,
    pub arg: u32,
    /// The first 32 bits of the last response (card status, OCR, RCA and status, or the echo of CMD8), 0 for none.
    pub response: u32,
    pub outcome: Outcome,
    /// How often it was sent (ACMD41 polls until the card is ready).
    pub repeats: u16,
    /// Microseconds over all repeats.
    pub us: u32,
}

/// A raw command, see [SdCard::command].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct RawCommand<'i> {
    /// Command index, 0..=63.
    pub index: u8,
    pub arg: u32,
    /// An application command: CMD55 with the card's RCA goes first.
    pub app: bool,
    pub response: ResponseType,
    pub data: DataPhase<'i>,
    /// How long to wait for the response, the data and the end of busy, in ms; `None`: the driver's default.
    pub timeout_ms: Option<u32>,
}

/// The response a command expects, see [RawCommand::response]. The controller needs it to receive the response.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum ResponseType {
    None,
    /// Card status.
    R1,
    /// Card status, then busy on DAT0.
    R1b,
    /// CID or CSD, 128 bits.
    R2,
    /// OCR, no CRC.
    R3,
    /// Published RCA and part of the card status.
    R6,
    /// Interface condition (CMD8).
    R7,
}

/// The data phase of a raw command, see [RawCommand::data].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq), ww_repr = nib)]
pub enum DataPhase<'i> {
    None,
    /// Read `blocks` blocks of `block_len` bytes (e.g. 64 for ACMD13 and CMD6, 8 for ACMD51, 512 for CMD17).
    Read {
        block_len: u16,
        blocks: u16,
    },
    /// Write `data` in blocks of `block_len` bytes.
    Write {
        block_len: u16,
        data: MaxLen<RefVec<'i, u8>, MAX_DATA>,
    },
}

/// What a raw command got back, see [SdCard::command].
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct RawResponse<'i> {
    pub outcome: Outcome,
    /// Response bytes, MSB first, without start bit, index and CRC: 4 for R1, R1b, R3, R6 and R7, 16 for R2 (its
    /// last byte as the controller delivered it); empty for none or when it didn't come.
    pub response: MaxLen<RefVec<'i, u8>, 16>,
    /// The card status bits of an R1 or R1b response (an R6's 16 status bits are mapped into it), `None` for other
    /// responses.
    pub status: Option<CardStatus>,
    /// Data read in the data phase, empty for none. On a data CRC error, what arrived.
    pub data: MaxLen<RefVec<'i, u8>, MAX_DATA>,
    pub timing: Timing,
}

/// How a command went on the bus, see [RawResponse::outcome] and [InitStep::outcome].
///
/// `Unsized` (default): a later version may add an outcome without breaking a host built against this one.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Outcome {
    /// The response (and the data phase) came without error.
    Ok,
    /// No response within the timeout.
    ResponseTimeout,
    /// The response's CRC7 failed (R3 has no CRC: a controller that checks it anyway reports this, the bytes are
    /// fine).
    ResponseCrc,
    /// The data phase didn't start or finish in time.
    DataTimeout,
    /// A data block's CRC16 failed, or the card refused written data (CRC status).
    DataCrc,
    /// The card stayed busy past the timeout.
    BusyTimeout,
    /// Another controller error, the device's code.
    Other { code: u8 },
}

/// Card status bits of an R1 response (SD Physical Layer spec, card status), see [RawResponse::status].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct CardStatus {
    pub bits: u32,
}

impl CardStatus {
    pub const OUT_OF_RANGE: u32 = 1 << 31;
    pub const ADDRESS_ERROR: u32 = 1 << 30;
    pub const BLOCK_LEN_ERROR: u32 = 1 << 29;
    pub const ERASE_SEQ_ERROR: u32 = 1 << 28;
    pub const ERASE_PARAM: u32 = 1 << 27;
    pub const WP_VIOLATION: u32 = 1 << 26;
    pub const CARD_IS_LOCKED: u32 = 1 << 25;
    pub const LOCK_UNLOCK_FAILED: u32 = 1 << 24;
    pub const COM_CRC_ERROR: u32 = 1 << 23;
    pub const ILLEGAL_COMMAND: u32 = 1 << 22;
    pub const CARD_ECC_FAILED: u32 = 1 << 21;
    pub const CC_ERROR: u32 = 1 << 20;
    pub const ERROR: u32 = 1 << 19;
    pub const CSD_OVERWRITE: u32 = 1 << 16;
    pub const WP_ERASE_SKIP: u32 = 1 << 15;
    pub const CARD_ECC_DISABLED: u32 = 1 << 14;
    pub const ERASE_RESET: u32 = 1 << 13;
    pub const READY_FOR_DATA: u32 = 1 << 8;
    pub const FX_EVENT: u32 = 1 << 6;
    pub const APP_CMD: u32 = 1 << 5;
    pub const AKE_SEQ_ERROR: u32 = 1 << 3;
    /// The error bits (31..=26, 24..=19, 16, 15, 3): any of them set means the command (or an earlier one) failed.
    pub const ERRORS: u32 = 0xfdf9_8008;

    /// `CURRENT_STATE` (bits 12:9): 0 idle, 1 ready, 2 ident, 3 stby, 4 tran, 5 data, 6 rcv, 7 prg, 8 dis.
    pub const fn current_state(&self) -> u8 {
        ((self.bits >> 9) & 0xf) as u8
    }

    /// Whether `bit` (one of the constants) is set.
    pub const fn has(&self, bit: u32) -> bool {
        self.bits & bit != 0
    }

    /// Any error bit set.
    pub const fn is_error(&self) -> bool {
        self.bits & Self::ERRORS != 0
    }
}

/// How long a command took, see [RawResponse::timing]. Microseconds from sending the command.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Timing {
    /// To the end of the response, `None` without one.
    pub response_us: Option<u32>,
    /// To the end of the data phase, `None` without one.
    pub data_us: Option<u32>,
    /// To the end of busy (R1b, writes, erases), `None` when the card wasn't busy.
    pub busy_us: Option<u32>,
}

/// What a raw command needs, see [RawCommand::access] and the crate docs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Access {
    /// Reads and status queries: at any time.
    Free,
    /// Changes the card's data or state: the lease of `ww_block`.
    Lease,
    /// Can lock or damage the card for good: refused without the feature `raw-sd-unrestricted`, and with it needs
    /// the lease.
    Restricted,
}

/// What a raw command needs, from its index, argument and whether it is an application command; see
/// [RawCommand::access]. An allow-list: only commands known to read are [Access::Free].
pub const fn access(index: u8, arg: u32, app: bool) -> Access {
    const SWITCH_SET: u32 = 1 << 31;
    if app {
        return match index {
            // SD_STATUS, SEND_NUM_WR_BLOCKS, SEND_SCR
            13 | 22 | 51 => Access::Free,
            _ => Access::Lease,
        };
    }
    match index {
        // LOCK_UNLOCK, PROGRAM_CSD (permanent write protect)
        42 | 27 => Access::Restricted,
        // SWITCH_FUNC in check mode only reads the function table
        6 if arg & SWITCH_SET == 0 => Access::Free,
        // SEND_CSD, SEND_CID, SEND_STATUS, READ_SINGLE/MULTIPLE_BLOCK, SEND_TUNING_BLOCK, SEND_WRITE_PROT, CMD55
        // itself, READ_OCR and CRC_ON_OFF of SPI mode
        9 | 10 | 13 | 17 | 18 | 19 | 30 | 55 | 58 | 59 => Access::Free,
        // GEN_CMD read (arg bit 0 set)
        56 if arg & 1 == 1 => Access::Free,
        _ => Access::Lease,
    }
}

impl RawCommand<'_> {
    /// What this command needs, see [access].
    pub const fn access(&self) -> Access {
        access(self.index, self.arg, self.app)
    }
}

#[cfg(feature = "std")]
impl RawCommandOwned {
    /// What this command needs, see [access]: a host can tell before sending it.
    pub const fn access(&self) -> Access {
        access(self.index, self.arg, self.app)
    }
}

/// The access rule of the crate docs for a device's [SdCard::command] handler: `lease_ok` is whether the call
/// carries the current `ww_block` lease. Restricted commands pass only in a build with `raw-sd-unrestricted`.
pub fn check_access(cmd: &RawCommand<'_>, lease_ok: bool) -> Result<(), SdError<'static>> {
    match cmd.access() {
        Access::Free => Ok(()),
        Access::Restricted if !UNRESTRICTED => Err(SdError::Restricted { index: cmd.index }),
        Access::Lease | Access::Restricted if lease_ok => Ok(()),
        Access::Lease | Access::Restricted => Err(SdError::BadLease),
    }
}

/// This build lets restricted commands through (the feature `raw-sd-unrestricted`), for [Capabilities::unrestricted].
pub const UNRESTRICTED: bool = cfg!(feature = "raw-sd-unrestricted");

/// Errors of the SD card calls.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    display,
    ww_repr = unib32
)]
pub enum SdError<'i> {
    /// Common error
    #[display("{0}")]
    #[common]
    Common(CommonError<'i>),
    /// No card in the slot.
    #[display("no card")]
    NoCard,
    /// The command changes the card and the call has no lease, or one that lapsed: claim the card with
    /// `ww_block::BlockDevice::claim`.
    #[display("needs the block device lease")]
    BadLease,
    /// The command can lock or damage the card for good and this device is built without `raw-sd-unrestricted`.
    #[display("CMD{index} is restricted on this device")]
    Restricted { index: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(index: u8, arg: u32, app: bool) -> RawCommand<'static> {
        RawCommand {
            index,
            arg,
            app,
            response: ResponseType::R1,
            data: DataPhase::None,
            timeout_ms: None,
        }
    }

    #[test]
    fn access_rules() {
        assert_eq!(cmd(17, 0, false).access(), Access::Free);
        assert_eq!(cmd(13, 0, true).access(), Access::Free);
        assert_eq!(cmd(6, 0x00ff_fff1, false).access(), Access::Free);
        assert_eq!(cmd(6, 0x80ff_fff1, false).access(), Access::Lease);
        assert_eq!(cmd(38, 0, false).access(), Access::Lease);
        assert_eq!(cmd(24, 0, false).access(), Access::Lease);
        assert_eq!(cmd(6, 2, true).access(), Access::Lease);
        assert_eq!(cmd(42, 0, false).access(), Access::Restricted);
        assert_eq!(cmd(27, 0, false).access(), Access::Restricted);
        // ACMD42 (card detect pull-up) is not CMD42
        assert_eq!(cmd(42, 0, true).access(), Access::Lease);
    }

    #[test]
    fn check_access_needs_lease() {
        assert_eq!(check_access(&cmd(17, 0, false), false), Ok(()));
        assert_eq!(
            check_access(&cmd(38, 0, false), false),
            Err(SdError::BadLease)
        );
        assert_eq!(check_access(&cmd(38, 0, false), true), Ok(()));
        let restricted = check_access(&cmd(42, 0, false), true);
        if UNRESTRICTED {
            assert_eq!(restricted, Ok(()));
        } else {
            assert_eq!(restricted, Err(SdError::Restricted { index: 42 }));
        }
    }

    #[test]
    fn card_status_bits() {
        // tran state, ready for data, app cmd
        let s = CardStatus {
            bits: (4 << 9) | CardStatus::READY_FOR_DATA | CardStatus::APP_CMD,
        };
        assert_eq!(s.current_state(), 4);
        assert!(!s.is_error());
        assert!(
            CardStatus {
                bits: CardStatus::ILLEGAL_COMMAND
            }
            .is_error()
        );
    }

    #[test]
    fn raw_command_round_trip() {
        let data = [0xa5u8; 512];
        let c = RawCommand {
            index: 24,
            arg: 0x1000,
            app: false,
            response: ResponseType::R1,
            data: DataPhase::Write {
                block_len: 512,
                data: MaxLen::new(RefVec::new_bytes(&data)).unwrap(),
            },
            timeout_ms: Some(250),
        };
        let mut buf = [0u8; 600];
        let bytes = c.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(RawCommand::from_ww_bytes(bytes).unwrap(), c);
    }
}
