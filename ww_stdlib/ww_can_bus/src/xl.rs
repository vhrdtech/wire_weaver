use wire_weaver::prelude::*;
use ww_si::Fixed;
use ww_si::prefix::One;
use ww_si::units;

use crate::config::{Tdc, Timing};
use crate::{CanError, TxMarker};

/// Largest data of an [XlFrame], in bytes.
#[cfg(feature = "xl")]
pub const MAX_XL_DATA_LEN: usize = 2048;

/// CAN XL frames of one controller (behind the `xl` cargo feature, so devices without XL don't carry its 2 KiB
/// frames): a device puts it next to its controllers, `ww_impl!(can_xl[]: ww_can_bus::CanXl)`, element `i` being
/// controller `i`. Classic and FD frames of the same controller stay on `Can::rx` / `Can::tx`.
#[cfg(feature = "xl")]
#[ww_trait]
#[claim(exclusive)]
pub trait CanXl {
    /// Configures the XL data phase (`None`: no XL frames) and answers what was set. Needs CAN FD configured on the
    /// controller (`WrongState`); a timing or transceiver mode switching it can't do is `Unsupported`.
    #[errors(Common(Unsupported, InvalidArgument, WrongState))]
    fn configure_xl(config: Option<XlConfig>) -> Result<Option<XlConfig>, CanError<'i>>;

    /// Received XL frames. A counted stream, unit: frame; the head's time is when the frame was received.
    #[counted]
    stream!(rx_xl: XlFrame<'i>);

    /// XL frames to send; credit-based. Reported on `Can::tx_events` with their marker, where present.
    #[credit]
    sink!(tx_xl: XlTxFrame<'i>);
}

/// A CAN XL frame.
#[cfg(feature = "xl")]
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct XlFrame<'i> {
    /// Priority ID (11 bits), arbitrated like a standard ID.
    pub priority: U11,
    /// Virtual CAN network ID; `None` when not used.
    pub vcid: Option<VcId>,
    /// SDU type: what the data carries (CiA 611-1).
    #[no_unit_hint = "a protocol code (CiA 611-1 SDU type), not a number"]
    pub sdt: u8,
    /// Simple extended content: CADH (CiA 613-2) present.
    pub sec: bool,
    /// Acceptance field (32 bits).
    #[no_unit_hint = "a protocol field the receivers' filters match (a bit pattern), not a number"]
    pub af: u32,
    /// 1-2048 bytes.
    pub data: MaxLen<RefVec<'i, u8>, MAX_XL_DATA_LEN>,
}

/// Virtual CAN network ID of an [XlFrame] (8 bits, ISO 11898-1:2024).
#[cfg(feature = "xl")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    transparent,
    derive(Copy, Clone, Debug, PartialEq, Eq, Hash)
)]
pub struct VcId(pub u8);

/// An XL frame to send, with a marker for `Can::tx_events`.
#[cfg(feature = "xl")]
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq, Eq))]
pub struct XlTxFrame<'i> {
    pub frame: XlFrame<'i>,
    pub marker: Option<TxMarker>,
}

/// XL data phase settings, see `CanXl::configure_xl`.
#[cfg(feature = "xl")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct XlConfig {
    /// Bit timing of the XL data phase.
    pub data: Timing,
    pub tdc: Tdc,
    /// Transceiver mode switching (CAN SIC XL transceivers): the data phase as PWM symbols; `None` for an SIC
    /// transceiver without switching (data phase up to about 5 Mbit/s).
    pub pwm: Option<XlPwm>,
    /// Error signalling in the data phase; off for PWM mode, where errors are left to higher layers.
    pub error_signaling: bool,
}

/// PWM symbol timing of the XL data phase, in minimum time quanta (clock periods).
#[cfg(feature = "xl")]
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct XlPwm {
    pub short: Fixed<units::Cycle, One, u8>,
    pub long: Fixed<units::Cycle, One, u8>,
    pub offset: Fixed<units::Cycle, One, u8>,
}
