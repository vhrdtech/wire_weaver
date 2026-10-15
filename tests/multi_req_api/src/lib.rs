//! API for multi requests (`MultiCall`, `MultiRead`, `MultiWrite`) and the malformed ones a server must survive.
use wire_weaver::prelude::*;

#[ww_trait]
trait MultiReq {
    property!(rw a: u8);
    property!(rw b: u16);
    /// `x + y`, wrapping
    fn add(x: u8, y: u8) -> u8;
    /// Answered later, refused inside a multi request
    fn deferred() -> u8;
    /// Defers although the method model says immediate, refused inside a multi request too
    fn defers_anyway() -> u8;
    /// Server returns Unimplemented
    fn absent();
    /// Pins 0..4
    ww_impl!(pin[]: Pin);
}

#[ww_trait]
trait Pin {
    /// Sets the pin's level
    fn set_level(high: bool);
    property!(rw mode: u8);
}
