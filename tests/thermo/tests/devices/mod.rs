//! Simulated firmware of each line, on `tests_common`'s in-process link.

use std::sync::{Arc, Mutex};

pub mod w1;
pub mod w1_0;
pub mod w2;
pub mod w2_0;

/// What a test reads back from a device.
#[derive(Default, Debug)]
pub struct State {
    /// Requests that reached the device
    pub requests: u32,
    /// The rate last set, Hz
    pub rate: u16,
}

pub type Shared = Arc<Mutex<State>>;
