//! Records of [LastReset](crate::LastReset).

use wire_weaver::prelude::*;
use ww_si::Second;

/// Why the device reset last and what was saved before it, see [LastReset::last_reset](crate::LastReset::last_reset).
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct ResetRecord<'i> {
    /// What the reset-cause register (or the crash record) says.
    pub cause: ResetCause,
    /// Build id of the firmware that crashed: the record may be older than an update. Empty when unknown.
    pub build_id: MaxLen<RefVec<'i, u8>, 32>,
    /// Device time of the crash since its boot.
    pub uptime: Option<Second>,
    /// Registers saved by the fault handler, for a fault or panic.
    pub fault: Option<Fault>,
    /// Top of the stack at the crash, innermost word first, up to 32 words. Hosts symbolize return addresses with
    /// the ELF found by `build_id`.
    pub stack: MaxLen<RefVec<'i, u32>, 32>,
    /// Panic message or fault text, cut at the bound.
    pub message: MaxLen<&'i str, 128>,
}

/// Cause of a reset.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq),
    ww_repr = unib32
)]
pub enum ResetCause {
    /// Power was applied.
    PowerOn,
    /// Supply voltage dropped below the brown-out threshold.
    Brownout,
    /// The reset pin was pulled low.
    Pin,
    /// The watchdog was not fed in time.
    Watchdog,
    /// Firmware asked for a reset (`sys_reset`).
    Software,
    /// The core locked up (a fault inside a fault handler).
    Lockup,
    /// A hardware fault, see `ResetRecord::fault`.
    Fault,
    /// A Rust panic, see `ResetRecord::message`.
    Panic,
    /// The reset that finished a firmware update.
    Updated,
    /// The device cannot tell.
    Undetermined,
}

/// Registers saved by a fault handler, one variant per architecture.
///
/// Variants exist behind the `cortex-m` and `riscv` features and keep their numbers without them, so a build with
/// one feature reads what a build with the other reports as `EnumVariantDisabled`. `std` enables all of them.
/// Needs at least one architecture feature.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq),
    non_exhaustive,
    ww_repr = unib32
)]
pub enum Fault {
    /// ARMv6-M to ARMv8-M cores (RP2040, RP2350 Cortex-M33, STM32).
    #[cfg(feature = "cortex-m")]
    CortexM(CortexMFault),
    /// RISC-V cores (RP2350 Hazard3, ESP32-C).
    #[cfg(feature = "riscv")]
    RiscV(RiscVFault),
}

/// Exception frame and fault status registers of a Cortex-M core.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq)
)]
pub struct CortexMFault {
    /// Program counter at the fault.
    pub pc: u32,
    /// Link register.
    pub lr: u32,
    /// Program status register.
    pub xpsr: u32,
    /// Stack pointer.
    pub sp: u32,
    /// Configurable fault status.
    pub cfsr: u32,
    /// Hard fault status.
    pub hfsr: u32,
    /// MemManage fault address.
    pub mmfar: u32,
    /// Bus fault address.
    pub bfar: u32,
    /// `EXC_RETURN` value of the exception entry.
    pub exc_return: u32,
}

/// Trap registers of a RISC-V core.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Copy, Debug, PartialEq, Eq)
)]
pub struct RiscVFault {
    /// Exception program counter.
    pub mepc: u32,
    /// Trap cause.
    pub mcause: u32,
    /// Trap value (faulting address or instruction).
    pub mtval: u32,
    /// Return address register.
    pub ra: u32,
    /// Stack pointer.
    pub sp: u32,
}
