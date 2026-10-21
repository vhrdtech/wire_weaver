use wire_weaver::prelude::*;

/// A device several clients share through the ww hub (SRV-29).
#[ww_trait]
trait HubTest {
    fn add(a: u32, b: u32) -> u32;
    stream!(ticks: u32);
    stream!(other: u8);
}
