//! API of the hub reconnect tests (CLT-40, SRV-30): calls to count, and a counted stream to resume.
use wire_weaver::prelude::*;

#[ww_trait]
trait HubReconnect {
    fn add(a: u32, b: u32) -> u32;
    /// Ticks counting up; unit: tick
    #[counted]
    stream!(ticks: u32);
    /// The device sends the next `count` ticks
    fn emit(count: u32);
}
