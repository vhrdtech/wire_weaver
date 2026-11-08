//! Simulated firmware of every line of the `thermo_api` bench (EVO-28), on `tests_common`'s in-process link: for
//! `tests/thermo`'s driver tests, the dynamic client's and Python's (`thermo_pytest_devices`, which hosts them behind a
//! ww hub).

use std::sync::{Arc, Mutex};

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
