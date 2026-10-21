use wire_weaver::prelude::*;

/// A device several clients share through the ww hub (SRV-29), with resources one of them can claim (SRV-34).
#[ww_trait]
trait HubTest {
    fn add(a: u32, b: u32) -> u32;
    stream!(ticks: u32);
    stream!(other: u8);
    /// Two buses, claimed one by one.
    #[claim]
    #[shares(index = 0, pins = "PA11, PA12")]
    #[label("CAN1, front", "CAN2, back")]
    ww_impl!(bus[]: Bus);
    /// A port on the pins of bus 0: claimable by its trait, overlaps `bus[0]`.
    #[shares(pins = "PA11")]
    ww_impl!(port: Port);
}

#[ww_trait]
trait Bus {
    /// Sends a frame: `Common(InUse)` from the hub while another program holds the bus.
    fn send(id: u32) -> Result<u32, BusError<'i>>;
    /// Bit rate.
    property!(rw speed: u32, BusError<'i>);
    /// Received frames.
    stream!(rx: u32);
}

#[ww_trait]
#[claim(observe)]
trait Port {
    fn ping() -> u8;
}

/// Errors of [Bus].
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
pub enum BusError<'i> {
    /// {0}
    #[common]
    Common(CommonError<'i>),
    /// The bus is off.
    BusOff,
}

/// A device whose clock the hub steers (SRV-31): `ww_timebase::Clock` with `host_sync`.
#[ww_trait]
trait ClockedTest {
    fn add(a: u32, b: u32) -> u32;
    ww_impl!(clock: ww_timebase::Clock);
}
