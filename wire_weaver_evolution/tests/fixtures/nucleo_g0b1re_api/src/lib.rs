//! WireWeaver API of the nucleo_g0b1re firmware (`../nucleo_g0b1re_api`), served by `../firmware/src/ww.rs`.
//!
//! The crate name and version identify the API on the wire (`NUCLEO_G_0_B_1_RE_API_FULL_GID`), so bump the version with every
//! change and follow the WireWeaver evolution rules to stay compatible with deployed devices and hosts:
//! https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md
//! Data types used in the API go here too, with `#[derive_shrink_wrap]`.
//!
//! 0.2 puts the device contract under `device` (`Device`, the same tree as Donguru and B135): firmware, last reset,
//! board, unique id, label, counters, log, variant, settings, boot control and the update node. Generic host tools
//! (`ww info`, `ww counters`, `ww log`, `ww dfu`) find it there.
//!
//! 0.3 adds `pins` (`ww_pin_diag::PinDiag`): the board's free pins for pin tests (`ww pins`).
//!
//! 0.4 adds `leds` (`ww_indication::Indication`: mode, brightness, identify; LD4 breathes by default) and `clock`
//! (`ww_timebase::Clock` with the `host_sync` group: a host steers the board onto UTC, so two boards breathe in
//! phase). `led_on` / `led_off` stay: LD4 solid on, off.
#![no_std]

use wire_weaver::prelude::*;

/// nucleo_g0b1re device
#[ww_api_root]
pub trait NucleoG0b1reApi {
    /// LD4 solid on (`leds` shows it until another mode)
    fn led_on();
    /// LD4 off (`leds` shows it until another mode)
    fn led_off();

    /// Device contract: firmware, last reset, board, ids, label, counters, log, variant, settings, boot, update node
    #[since = "0.2.0"]
    ww_impl!(device: Device);

    /// Raw access to the pins the board leaves free (docs/board/nucleo_g0b1re-pins.md), for bring-up and pin tests
    /// (`ww pins tui`): one program at a time, every pin given back when it lets go or disconnects
    #[since = "0.3.0"]
    ww_impl!(pins: ww_pin_diag::PinDiag);

    /// LD4 as one indication: global mode, brightness, identify. It breathes by default, in phase with other boards
    /// once `clock` is steered.
    #[since = "0.4.0"]
    ww_impl!(leds: ww_indication::Indication);
    /// Device clock: local time (U32 µs, wraps every 71.6 minutes), a host's steer points onto UTC (`host_sync`).
    /// No sync line.
    #[since = "0.4.0"]
    ww_impl!(clock: ww_timebase::Clock);
}

/// The device contract: what every WireWeaver device answers, so generic host tools (`ww info`, `ww log`) work.
#[ww_trait]
pub trait Device {
    /// Which firmware runs and its build id
    ww_impl!(firmware: ww_firmware_info::FirmwareInfo);
    /// Why the device last reset, with the crash record after a fault or panic, and the boot count
    ww_impl!(last_reset: ww_firmware_info::LastReset);
    /// Board name, revision and serial number
    ww_impl!(board: ww_board_info::BoardInfo);
    /// STM32 96-bit unique id
    ww_impl!(uid: ww_uid::Uid);
    /// Name the user gives this unit, kept in flash
    ww_impl!(label: ww_user_info::UserInfo);
    /// `cnt` counters: RAM and backup registers
    ww_impl!(counters: ww_counters::Counters);
    /// The defmt log, also kept while no host is connected
    ww_impl!(log: ww_log::LogDefmt);
    /// The chip on this board, read from the chip itself: device id, silicon revision, flash size
    property!(ro variant: Variant);
    /// Settings kept in flash
    ww_impl!(settings: Settings);
    /// Boot state, confirm, revert, reboot: `reboot(Updater)` goes to the STM32 ROM bootloader. The state is always
    /// `Confirmed`, there is no image to revert to.
    ww_impl!(boot: ww_dfu::BootControl);
    /// The update node: the firmware in place, written by the ROM bootloader (`Profile::InPlace`, `device.boot` only)
    ww_impl!(dfu: Dfu);
}

/// Firmware update over the API (`ww_dfu`): here only who the node is. Updates go through the STM32 ROM bootloader
/// (`device.boot.reboot(Updater)`, then `ww dfu stm32 flash` over USB on PA11/PA12), or through the debug probe.
#[ww_trait]
pub trait Dfu {
    /// Who this node is: product, profile, boot state, last result
    ww_impl!(node: ww_dfu::UpdateNode);
}

/// Settings kept in flash across power cycles. A write is in flash before the call answers. The label is
/// `device.label`, kept the same way.
#[ww_trait]
pub trait Settings {
    /// The LED after boot: on (`true`) or off, default off
    #[errors(Fault)]
    property!(rw led_at_boot: bool, CommonError<'i>);
    /// Every setting back to its default, the label too
    #[errors(Fault)]
    fn reset() -> Result<(), CommonError<'i>>;
}

/// The chip a board carries, see [`Device::variant`]: every Nucleo-G0B1RE has an STM32G0B1RET6, what can differ is
/// its silicon revision (and a G0C1 fitted by hand).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Variant {
    /// DBGMCU_IDCODE.DEV_ID: 0x467 for STM32G0B1 / G0C1
    pub dev_id: u16,
    /// DBGMCU_IDCODE.REV_ID, the raw silicon revision code
    pub rev_id: u16,
    /// The silicon revision `rev_id` stands for (ES0548)
    pub revision: SiliconRevision,
    /// Flash size in KiB (FLASH_SIZE register): 512 on the G0B1RE
    pub flash_kib: u16,
}

/// Silicon revision of an STM32G0B1 / G0C1 as ES0548 lists them, see [`Variant::rev_id`]. A host reads one it
/// doesn't know as [`Other`](SiliconRevision::Other).
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Copy, Clone, Debug, PartialEq, Eq), ww_repr = nib, future_version = Other)]
pub enum SiliconRevision {
    /// Revision A, REV_ID 0x1000
    A,
    /// A REV_ID this firmware doesn't know (a revision newer than it)
    Unknown,
}

impl SiliconRevision {
    /// The revision of a DBGMCU_IDCODE.REV_ID value.
    pub const fn from_rev_id(rev_id: u16) -> Self {
        match rev_id {
            0x1000 => SiliconRevision::A,
            _ => SiliconRevision::Unknown,
        }
    }
}
