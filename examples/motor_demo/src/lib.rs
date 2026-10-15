//! Motor controller demo (EX-11): a VESC-like FOC [controller] driving a simulated motor (`motor_sim`), exposed as a
//! wire_weaver device ([device]) with its ~50 variables as `+observe` properties in three rate tiers, and the host
//! side bookkeeping of what observing them costs ([stats]).

pub mod client;
pub mod controller;
pub mod device;
pub mod host;
pub mod stats;
