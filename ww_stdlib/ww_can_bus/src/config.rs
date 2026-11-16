use ww_api::prelude::*;
use ww_si::prefix::One;
use ww_si::units;
use ww_si::{Baud, Fixed, Hertz, Overflow, Prefix, Rounding, Second};

#[expect(
    unused_imports,
    reason = "only resolves the `Capabilities::clock` doc link below"
)]
use crate::capabilities::Capabilities;
use crate::{Prescaler, Share};

/// Everything a controller is configured with, see `Can::configure`: what is asked for going in, what was set
/// coming back.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct CanConfig {
    pub mode: Mode,
    /// Bit timing of the arbitration phase (all of a classic frame).
    pub nominal: Timing,
    /// CAN FD: `None` sends and accepts classic frames only.
    pub fd: Option<FdConfig>,
    pub options: Options,
    /// Recover from bus-off by itself this long after entering it (zero: at once); `None`: stay bus-off until
    /// `Can::recover`. A device that can't do one or the other answers `Unsupported`.
    pub bus_off_recovery: Option<Second>,
}

/// Operating mode of a controller.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Mode {
    /// Powered down (clock gated where the controller can be alone), off the bus, TX recessive.
    Off,
    /// Off the bus, TX recessive, everything configurable (M_CAN `INIT` + `CCE`, bxCAN initialization, MCP2518FD
    /// configuration mode); the mode to stay in for configuration-only steps.
    Config,
    /// Sends, receives and acknowledges.
    Normal,
    /// Receives and acknowledges, but doesn't send frames or active error / overload flags (ISO 11898-1
    /// restricted operation); error counters are frozen.
    Restricted,
    /// Receives only, sends nothing at all, not even an acknowledge (bus monitoring, silent, listen-only).
    ListenOnly,
    /// Sent frames are received back internally; the bus is not driven and not read.
    InternalLoopback,
    /// Sent frames go on the bus and are received back, acknowledged or not.
    ExternalLoopback,
    /// Production test: `Can::test_pins` drives TX and reads RX.
    Test,
    /// Low power, off the bus; wakes up on bus activity with [Options::wake_on_bus], or on `Can::set_mode`.
    Sleep,
}

/// Bit timing of one phase: asked for as a bit rate or as segments, answered as segments.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = nib
)]
pub enum Timing {
    /// A bit rate and sample point, the device picks the segments.
    Rate {
        rate: Baud,
        /// Sample point, a share of the bit (875 ‰ = 87.5 %).
        sample_point: Share,
    },
    /// Segments in time quanta of `prescaler / Capabilities::clock`.
    Segments(BitTiming),
}

impl Timing {
    /// The segments, `None` for a `Rate`.
    pub fn segments(&self) -> Option<BitTiming> {
        match self {
            Timing::Segments(bt) => Some(*bt),
            _ => None,
        }
    }
}

/// Bit timing in time quanta: a bit is `1 + seg1 + seg2` quanta of `prescaler` clock periods each, sampled after
/// `1 + seg1` (seg1 = propagation + phase segment 1, without the sync quantum, as SocketCAN and the M_CAN `NBTP`
/// count).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct BitTiming {
    pub prescaler: Prescaler,
    pub seg1: Fixed<units::TimeQuantum, One, u16>,
    pub seg2: Fixed<units::TimeQuantum, One, u8>,
    /// Synchronization jump width.
    pub sjw: Fixed<units::TimeQuantum, One, u8>,
}

impl BitTiming {
    /// Time quanta per bit.
    pub fn quanta(&self) -> Fixed<units::TimeQuantum, One, u32> {
        Fixed::new(1 + self.seg1.get() as u32 + self.seg2.get() as u32)
    }

    /// Bit rate at controller clock `clock` ([Capabilities::clock]), rounded down to a whole bit/s; `None` for a
    /// zero prescaler or a clock of 2³² Hz and above.
    pub fn bit_rate(&self, clock: Hertz) -> Option<Fixed<units::Baud, One, u32>> {
        let clock: u32 = clock.to_in(Prefix::Unit, Rounding::Truncate, Overflow::Checked)?;
        let div = (self.prescaler.get() as u32).checked_mul(self.quanta().get())?;
        clock.checked_div(div).map(Fixed::new)
    }

    /// Sample point, a share of the bit.
    pub fn sample_point(&self) -> Share {
        let sp = (1 + self.seg1.get() as u32) * 1000 / self.quanta().get();
        // seg2 >= 0, so the sample point is at most the whole bit
        Share::try_new(sp as u16).unwrap_or(Share::try_new(1000).unwrap())
    }
}

/// CAN FD settings, see [CanConfig::fd].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq)
)]
pub struct FdConfig {
    /// Bit timing of the data phase of frames with bit rate switch; the same as the nominal one when no frame
    /// switches.
    pub data: Timing,
    /// Transceiver delay compensation, needed from about 2 Mbit/s in the data phase.
    pub tdc: Tdc,
    /// ISO 11898-1:2015 CAN FD (with the stuff count); `false`: the original Bosch (non-ISO) CAN FD.
    pub iso: bool,
}

/// Transceiver delay compensation: where the controller checks a bit it sent in the data phase.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Tdc {
    Off,
    /// The device measures the delay and sets the offset at the data sample point.
    Auto,
    /// Secondary sample point at the measured delay plus `offset`, in minimum time quanta (clock periods of
    /// [Capabilities::clock]); `filter`: shortest delay accepted as measured.
    Manual {
        offset: Fixed<units::Cycle, One, u8>,
        filter: Option<Fixed<units::Cycle, One, u8>>,
    },
}

/// Options of a controller, see [CanConfig::options]. A device answers `Unsupported` for one it doesn't have
/// ([Capabilities]), except where the value asked for is what it always does.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Options {
    /// Send each frame once, no automatic retransmission after an error or lost arbitration.
    pub one_shot: bool,
    /// Treat a missing acknowledge as received (self test on a bus without other nodes).
    pub presume_ack: bool,
    /// Sample each bit three times (classic, nominal bit rate).
    pub triple_sample: bool,
    /// Wait two bit times after each sent frame, so other nodes of the same priority get the bus.
    pub tx_pause: bool,
    /// Order of the `tx` queue.
    pub tx_order: TxOrder,
    /// Which frames a full receive FIFO keeps.
    pub rx_overflow: RxOverflow,
    /// Hard synchronization only on two dominant time quanta after a recessive one (M_CAN `CCCR.EFBI`), against
    /// glitches on long buses.
    pub edge_filtering: bool,
    /// Protocol exception handling (ISO 11898-1): on a frame of a newer protocol, wait for bus idle instead of
    /// signalling an error. On by default; off for buses where every such frame is an error.
    pub protocol_exception: bool,
    /// In [Mode::Sleep], wake up on bus activity.
    pub wake_on_bus: bool,
}

/// Order the frames of `Can::tx` go on the bus.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum TxOrder {
    /// As queued.
    Fifo,
    /// Lowest ID first (as the bus would arbitrate them).
    Priority,
}

/// Which frames a full receive FIFO keeps; the lost ones are counted in the `rx` head either way.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    sized,
    ww_repr = u1,
    no_future_version
)]
pub enum RxOverflow {
    /// Keeps the frames in the FIFO, drops the new one.
    KeepOldest,
    /// Overwrites the oldest frame with the new one.
    KeepNewest,
}
