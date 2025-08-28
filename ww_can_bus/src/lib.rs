use wire_weaver::prelude::*;
use ww_numeric::SubTypeKind;

/// Represents one or more CAN Bus interfaces with one clock and power source and possibly sharing memory
/// between interfaces as well. This trait mimics real hardware implementations found on e.g., STM32
/// microcontrollers where all instances belong to the same clock and power domain. So it's impossible
/// to change the clock or put one instance into power down mode without affecting the others.
#[ww_trait]
pub trait CANBusInterfaceGroup {
    /// One or more interfaces providing actual frame sending and receiving capabilities as well as termination control.
    ww_impl!(interface[]: CANBusInterface);

    /// Disable clock for all interfaces, each interface must already be in [powered down mode](CANMode::PoweredDown),
    /// otherwise the [error](CANError::NotInPowerDown) is returned. All interfaces will be put into [disabled mode](CANMode::Disabled).
    fn disable() -> Result<(), CANError>;

    /// Enable a chosen or default clock for all interfaces in a group. Note that it's not required to actually use all available interfaces,
    /// unused ones will stay in [powered down mode](CANMode::PoweredDown) mode.
    /// All interfaces will be put into [powered down mode](CANMode::PoweredDown).
    /// [Error](CANError::NotInDisabled) is returned if tried to call this method in incorrect state.
    fn enable() -> Result<(), CANError>;

    /// Return a list of possible clocks sources. It is up to the implementation to return an empty list
    /// or a list of length 1 if only 1 clock source is supported.
    fn clock_sources() -> RefVec<'i, &'i str>;

    /// Selects a valid clock source or returns the [error](CANError::UnsupportedClockSource)
    /// If any instance is not in [powered down mode](CANMode::PoweredDown), returns [error](CANError::NotInPowerDown).
    /// Implementation must use a default clock source and operate as if this method where called.
    fn set_clock_source(source: &'i str) -> Result<(), CANError>;

    /// Returns possible clock values in u32 Hz (range or list).
    fn valid_clocks() -> SubTypeKind<'i>;

    /// Selects a valid clock frequency or returns the [error](CANError::UnsupportedFrequency).
    /// If any instance is not in [powered down mode](CANMode::PoweredDown), returns [error](CANError::NotInPowerDown).
    fn set_clock(frequency: u32) -> Result<(), CANError>;

    /// Performance metrics for the whole group. Can be None if each interface is independent of others and system, and this
    /// metric would have been a simple sum.
    fn group_performance() -> Option<CANGroupPerformance>;
}

pub enum CANMode {
    /// An interface group is completely disabled, including any clocks. This is the lowest possible power state.
    Disabled,

    /// An interface group clock is enabled, but an interface itself is powered down.
    PoweredDown,

    /// Allows for the configuration of an interface. In any other modes configuration cannot be changed.
    Config,

    /// This mode can be used for a “Hot Self-test” meaning the CAN interface can be tested without
    /// affecting a running CAN system connected to the (FD)CAN_TX and (FD)CAN_RX pins. In this
    /// mode, (FD)CAN_RX pin is disconnected from the CAN controller and (FD)CAN_TX pin is held
    InternalLoopback,

    /// This mode is provided for hardware self-test. To be independent of external stimulation,
    /// the (FD)CAN ignore acknowledge errors (recessive bit sampled in the acknowledgement slot of a
    /// data / remote frame) in Loop Back mode. In this mode, the (FD)CAN perform internal
    /// feedback from its transmitted output to its receiver input. The (FD)CAN disregard the actual value of the (FD)CAN_RX
    /// input pin. The transmitted messages can be monitored at the (FD)CAN_TX transmit pin.
    ExternalLoopback,

    /// Node is able to transmit and receive data and remote frames and give acknowledgement to valid frames.
    Normal,

    /// In Restricted operation mode, the node is able to receive data and remote frames and to give
    /// acknowledgement to valid frames, but it does not send data frames, remote frames, active error
    /// frames, or overload frames. In case of an error condition or overload condition, it does not
    /// send dominant bits, instead it waits for the occurrence of bus idle condition to resynchronize
    /// itself to the CAN communication. The error counters for transmitting and receive are frozen while
    /// error logging (can_errors) is active.
    /// This mode is useful for nodes that need to passively receive information but should not interfere
    /// with other nodes transmissions or cause errors on the bus.
    Restricted,

    /// In Bus monitoring mode (for more details refer to ISO11898-1, 10.12 Bus monitoring),
    /// the (FD)CAN is able to receive valid data frames and valid remote frames, but cannot start a
    /// transmission. In this mode, it sends only recessive bits on the CAN bus. If the (FD)CAN is
    /// required to send a dominant bit (ACK bit, overload flag, active error flag), the bit is
    /// rerouted internally so that the controller can monitor it, even if the CAN bus remains in recessive
    /// state. In Bus monitoring mode, the TXBRP register is held in reset state. The Bus monitoring
    /// mode can be used to analyze the traffic on a CAN bus without affecting it by the transmission
    /// of dominant bits.
    BusMonitoring,

    /// Test mode must be used for production tests or self-test only. The software control for
    /// (FD)CAN_TX pin interferes with all CAN protocol functions. It is not recommended to use test
    /// modes for application.
    Test,
}

pub enum CANError {
    NotInDisabled,
    /// Returned if tried to call any configuration method of an interface group,
    /// while one or more interfaces where not in [powered down mode](CANMode::PoweredDown) mode.
    NotInPowerDown,
    UnsupportedFrequency,
    UnsupportedClockSource,
}

/// Represents one CAN Bus controller, it's PHY and optional termination resistor / mechanical switch or relay.
/// Can be part of a group or stand-alone depending on actual SoC/FPGA implementation.
#[ww_trait]
pub trait CANBusInterface {
    // timings
    // filters

    // TX FIFO or Queue
    // CANTxEnvelope without timestamp or use timestamp to transmit later? and also in TT?
    sink!(tx_buffer: CANEnvelope<'i>); // CANTxEnvelope with marker for saving to SD card to see later what it was?

    // Dedicated TX buffer
    //fn tx_write_dedicated(slot: u16, frame: CANFrame<'i>) -> Result<(), E>;
    //fn tx_read_dedicated(slot: u16) -> Result<CANFrame<'i>, E>;
    //fn tx_dedicated_pend(slot: u16) -> Result<(), E>;
    //fn tx_dedicated_periodic(slot: u16, period: Microseconds) -> Result<(), E>;

    // RX FIFO or Queue 0
    stream!(rx_buffer0: CANEnvelope<'i>);
    // RX FIFO or Queue 1
    stream!(rx_buffer1: CANEnvelope<'i>);
    // Dedicated RX buffer
    stream!(rx_dedicated: CANEnvelope<'i>); // need slot here and don't need ID

    /// Returns the list of supported termination values, in Ohms.
    /// Must return an empty list if fixed value is set in hardware or via mechanical switch.
    fn supported_termination_values() -> RefVec<'i, u16 /*Ohms*/>;

    /// Returns current termination value, if any.
    /// This method must return Some(value) if it is fixed in hardware or via mechanical switch.
    fn current_termination_value() -> Option<u16>;

    /// Tries to set termination value to the requested one.
    fn set_termination_value(resistance: u16) -> CANTerminationSwitchResult;

    /// Interface capabilities
    fn capabilities() -> CANCapabilities;

    /// Interface performance metrics. Can be None if group one is Some - interfaces performance depend on each other, so only
    /// the combined performance makes sense.
    fn performance() -> Option<CANInterfacePerformance>;
}

pub struct CANEnvelope<'i> {
    pub frame: CANFrame<'i>,
    pub timestamp_us: Option<u32>, // since tick or absolute?
    pub timestamp_ns: Option<U30>,
}

#[derive_shrink_wrap]
#[derive(Debug, PartialEq, Eq)]
struct CANFrame<'i> {
    id: CANId,
    kind: CANFrameKind,
    // dir: Dir,
    data: RefVec<'i, u8>,
}

#[derive_shrink_wrap]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[ww_repr(u2)]
#[sized]
enum CANId {
    Standard(U11),
    Extended(U29),
}

#[derive_shrink_wrap]
#[derive(Debug, PartialEq, Eq)]
#[ww_repr(u2)]
#[sized]
enum CANFrameKind {
    Classic { rtr: CANRtr },
    FD { brs: CANBrs, esi: CANEsi },
}

#[derive_shrink_wrap]
#[derive(Debug, PartialEq, Eq)]
#[ww_repr(u1)]
#[sized]
#[derive(Copy, Clone)]
pub enum CANRtr {
    DataFrame,
    RemoteFrame,
}

#[derive_shrink_wrap]
#[derive(Debug, PartialEq, Eq)]
#[ww_repr(u1)]
#[sized]
#[derive(Copy, Clone)]
pub enum CANBrs {
    SameSpeed,
    SwitchForDataPhase,
}

#[derive_shrink_wrap]
#[derive(Debug, PartialEq, Eq)]
#[ww_repr(u1)]
#[sized]
#[derive(Copy, Clone)]
pub enum CANEsi {
    Normal,
    ErrorPassive,
}

pub struct CANCapabilities {
    pub eleven_bit_filters: u16,
    pub twenty_nine_bit_filters: u16,
    pub rx_fifo0: u16,
    pub rx_fifo1: u16,
    pub dedicated_rx_buffers: u16,
    pub tx_event_fifo: u16,
    pub dedicated_tx_buffers: u16,
    pub trigger_memory: u16,
    /// Whether the interface supports changing its RAM layout to get more dedicated buffer slots or change queue lengths
    pub dynamic_layout: bool,
    pub fd: bool,
    pub timestamping: bool,
    pub xl: bool,
    pub tt: bool,
}

pub enum CANTerminationSwitchResult {
    /// If value was actually just switched in.
    Ok,
    /// If requested value is already switched in.
    /// FixedInHardware must be returned even if the same value is being requested.
    Unchanged,
    /// If value is fixed in hardware or via mechanical switch (even if the same as requested).
    FixedInHardware,
    UnsupportedValue,
}

pub struct CANInterfacePerformance {
    /// Minimum guaranteed frames per second
    pub min_fps: Option<u32>,
    /// Minimum guaranteed bytes per second
    pub min_bps: Option<u32>,
    /// Whether one CAN instance performance is completely independent of others or not
    pub independent_from_other: bool,
    /// Whether performance is completely independent of other system activity or not
    pub independent_from_system: bool,
}

pub struct CANGroupPerformance {
    /// Minimum guaranteed frames per second across all interfaces
    pub min_group_fps: Option<u32>,
    /// Minimum guaranteed bytes per second across all interfaces
    pub min_group_bps: Option<u32>,
    /// Whether performance is completely independent of other system activity or not
    pub independent_from_system: bool,
}

pub enum CANNotificationKind {
    Timestamp,
    DeviceTime {
        rcc_seconds: u32,
        rcc_microseconds: u16,
    },
    CanErrorCounters {
        tec: u8,
        rec: u8,
    },
    BusOff,
    CanDisabled,
    CanSynchronizing,
    ProtocolErrorArbitrationPhase {
        code: CANErrorCode,
    },
    ProtocolErrorDataPhase {
        code: CANErrorCode,
    },
    TxFifoMessageLost,
    RxFifoMessageLost,
    /// Performance metrics should be evaluated again, because they changed due to system load or capabilities change
    PerformanceChanged,
}

pub enum CANErrorCode {
    Stuff,
    Form,
    Ack,
    Bit1,
    Bit0,
    CRC,
}
