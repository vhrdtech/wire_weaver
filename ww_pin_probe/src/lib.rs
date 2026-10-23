//! Host side of [`ww_pin_diag`] (STD-47, TOOL-15): finds shorts to GND, to the supply and between points, external
//! pull resistors and floating pins on any device serving `PinDiag`, wherever in its API.
//!
//! * [select::select] turns a list like `GPIO0,P8,GPIO2..=GPIO7,chain.*` into point ids.
//! * [plan::Plan] is a plan file: expectations per point, narrower allow lists, which points are one net.
//! * [probe::run] does the tests through [Pins] and gives every point back with `release_all` at the end, also when
//!   cancelled or on an error.
//! * [device::DevicePins] is [Pins] over a connected device, found through its introspection data.
//!
//! The tests only go through [Pins], so they run against a mock board as well as a device:
//!
//! ```ignore
//! let device = DynClient::from_config(config).connect().await?;
//! let mut pins = DevicePins::find(&device, None)?;
//! let names = Names::for_device(&device, pins.raw_points()).await;
//! let points = pins.points(&names);
//! let setup = Setup::new(&points, &select(&points, &["GPIO0..=GPIO7"])?, &[], None)?;
//! let report = run(&mut pins, &setup, tokio::signal::ctrl_c(), |_, _| {}).await?;
//! ```

use std::fmt::{Display, Formatter};
pub use ww_pin_diag::{Drive, Level, PinDiagErrorOwned, PointCaps, PointState, Pull};

pub mod board;
pub mod device;
pub mod live;
pub mod mock;
pub mod plan;
pub mod probe;
pub mod select;

/// One point of the device, its name and group as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Point {
    /// Index in the device's `POINTS`, the id in `set` and `read`.
    pub id: u16,
    pub name: String,
    pub group: String,
    pub caps: PointCaps,
}

/// Why a `set` or `read` didn't happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinError {
    /// The device said no: the point can't do it, a function holds it, or the board's rules forbid it now. Shown,
    /// and the tests go on.
    Refused(PinDiagErrorOwned),
    /// The call didn't go through (link, timeout, the device doesn't implement it): the tests stop.
    Failed(String),
}

impl Display for PinError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PinError::Refused(e) => write!(f, "{}", refusal(e)),
            PinError::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for PinError {}

/// A device's refusal in a few words: `not capable`, `in use by indication`, `refused: two VREF sources`.
pub fn refusal(e: &PinDiagErrorOwned) -> String {
    use wire_weaver::prelude::CommonErrorOwned;
    match e {
        PinDiagErrorOwned::NoSuchPoint => "no such point".into(),
        PinDiagErrorOwned::NotCapable => "not capable".into(),
        PinDiagErrorOwned::Refused { rule } => format!("refused: {rule}"),
        PinDiagErrorOwned::Common(CommonErrorOwned::InUse { by }) => {
            format!("in use by {}", by.as_str())
        }
        other => other.to_string(),
    }
}

/// Access to a board's points: what [probe::run] needs, implemented by [device::DevicePins] for a device and by a
/// mock board in tests.
// The tests are generic over this and awaited in place, never spawned, so the futures need not be Send.
#[allow(async_fn_in_trait)]
pub trait Pins {
    /// Sets one point, see `PinDiag::set`.
    async fn set(&mut self, point: u16, state: PointState) -> Result<(), PinError>;
    /// Level on one point's pad, see `PinDiag::read`.
    async fn read(&mut self, point: u16) -> Result<Level, PinError>;
    /// Every point back to its function, see `PinDiag::release_all`.
    async fn release_all(&mut self) -> Result<(), PinError>;
}
