//! Host clock sync for WireWeaver devices (STD-49): every device on the host's UTC.
//!
//! The code lives in [wire_weaver_client::clock_sync] (its `clock_sync` feature, on by default), where every direct
//! connection steers its device by itself (CLT-46); this crate re-exports it, so programs that steer with
//! [ClockSync] themselves keep working.

pub use wire_weaver_client::clock_sync::*;
