//! Simulated firmware of every line of the `thermo_api` bench (EVO-28), on `tests_common`'s in-process link: for
//! `tests/thermo`'s driver tests, the dynamic client's and Python's (`thermo_pytest_devices`, which hosts them behind a
//! ww hub).

use std::sync::{Arc, Mutex};
use wire_weaver::ww_version::WireVersion;

pub mod w1;
pub mod w1_0;
pub mod w2;
pub mod w2_0;
pub mod w3;

/// What a test reads back from a device.
#[derive(Default, Debug)]
pub struct State {
    /// Requests that reached the device
    pub requests: u32,
    /// The rate last set, Hz
    pub rate: u16,
}

pub type Shared = Arc<Mutex<State>>;

/// The bench's line of `wire`, 1 to 3, as `thermo_api`'s `wire_v1` to `wire_v3` name them.
///
/// # Panics
///
/// When `wire` is on none of them.
pub fn bench_line(wire: WireVersion) -> u16 {
    [
        thermo_api::wire_v1::THERMO_WIRE_VERSION,
        thermo_api::wire_v2::THERMO_WIRE_VERSION,
        thermo_api::wire_v3::THERMO_WIRE_VERSION,
    ]
    .iter()
    .zip(1..)
    .find_map(|(w, line)| w.same_line(&wire).then_some(line))
    .unwrap_or_else(|| panic!("{wire} is not a line of the bench"))
}
