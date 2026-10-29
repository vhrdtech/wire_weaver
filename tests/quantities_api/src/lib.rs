//! `ww_si` quantities in an API, as dynamic clients see them (CLT-31): a named unit, a `Series` of `Timed` values, a
//! compound unit (`Per`), a unit marker of this crate, declared with `unit!` so clients know it (INT-13), and a value whose unit is only known
//! at run time (`AnyQuantity`, STD-37), and values with unit, prefix and storage fixed at compile time (`Fixed`,
//! STD-55), written with and without their default bounds, and points against an origin of this crate and an angle
//! (STD-64). `Fixed` values, indices and sequence numbers as clients show, parse and check them (CLT-54): a property, a
//! method argument and return, a stream item.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::Series;
use ww_si::origins::{Angle, Celsius};
use ww_si::prefix::{Centi, FullScale, Kibi, Micro, Milli, One, Product, Quotient, Rational};
use ww_si::units::{
    self, AngularVelocity, Byte, Degree, Frame, Item, Meter, Per, Ratio, Sample, Second, Times,
    Torque, Watt,
};
use ww_si::{
    Ampere, AnyQuantity, Fixed, Index, IndexFrom1, Micros, Offset, Point, Quantity, Range,
    RangeDec, Seq, Timed, Unbounded, Volt,
};

#[ww_trait]
trait Quantities {
    property!(rw voltage: Volt);
    /// A supply rail, `0 V ..= 3.3 V` whatever prefix a value is sent with (STD-59)
    property!(rw rail: Quantity<units::Volt, RangeDec<0, 33, -1>>);
    /// A duty cycle in steps of 1/65535: an `Exact` prefix (STD-59)
    property!(ro duty: Quantity<Ratio>);
    /// Current history with device time
    fn history() -> Series<'i, Timed<Ampere>>;
    /// Viscous damping: torque per angular velocity
    property!(rw damping: Quantity<Per<Torque, AngularVelocity>>);
    property!(rw speed: Quantity<Rpm>);
    /// A sensor input whose unit is set by configuration, sent with every value (STD-37)
    property!(rw sensor: AnyQuantity<'i>);
    /// `B` left out: the walker fills in `Unbounded`
    property!(rw timeout: Fixed<Second, Milli, u16>);
    /// The same type with `B` written out
    property!(rw timeout_full: Fixed<Second, Milli, u16, Unbounded>);
    property!(rw window: Fixed<Second, Milli, u16, Range<1, 5000>>);
    /// Sizes with device time, `B` left out inside `Timed` and a `Series`
    fn sizes() -> Series<'i, Timed<Fixed<Byte, One, u32>>>;
    /// Through an alias, `B` left out
    fn ticks() -> Series<'i, Timed<Micros<u32>>>;
    /// A 12-bit ADC at 3.3 V: one step is 3.3/4096 V
    property!(ro adc: Fixed<units::Volt, FullScale<Rational<33, 10>, 12>, u16>);
    /// A gyro at 70 mdps per step, a scale marker of this crate (INT-13)
    property!(ro gyro: Fixed<DegreePerSecond, Mdps70, i16>);
    /// A speed written the long way round: (1/s)·m, the same unit as m/s
    property!(ro belt: Fixed<Times<Per<Ratio, Second>, Meter>, Milli, u16>);
    /// A board's temperature register, an origin of this crate (INT-13): raw / 256 + 25 °C
    property!(ro board_temp: Point<BoardTemp, Fixed<units::Kelvin, Rational<1, 256>, i16>>);
    /// A heading in 0.01° that wraps at 360°
    property!(ro heading: Point<Angle<Degree>, Fixed<Degree, Centi, u16>>);
    /// Time since boot in µs: past what an `f64` holds exactly
    property!(rw uptime: Fixed<Second, Micro, u64>);
    /// A buffer's size, in a binary prefix
    property!(rw buffer: Fixed<Byte, Kibi, u16>);
    /// Waits `delay`, at most 5 s, and returns it in ms
    fn wait(delay: Fixed<Second, Micro, u32, Range<0, 5000000>>) -> Fixed<Second, Milli, u32>;
    /// ADC readings in mV
    stream!(samples: Fixed<units::Volt, Milli, i16>);
    /// The input, 1 to 4 as printed on the board
    property!(rw input: IndexFrom1<Item, u8, Range<1, 4>>);
    /// Position in the capture buffer
    property!(rw sample: Index<Sample, u32>);
    /// The last frame's sequence number
    property!(rw frame: Seq<Frame, u16>);
    /// mV times mA, as `Fixed * Fixed` gives it (STD-83): one step is 1 µW, the scale `Product<Milli, Milli>`
    property!(ro power: Fixed<Watt, Product<Milli, Milli>, i32>);
    /// mV per µA, as `Fixed / Fixed` gives it (STD-83): one step is 1 kV/A, the scale `Quotient<Milli, Micro>`
    property!(ro transimpedance: Fixed<Per<units::Volt, units::Ampere>, Quotient<Milli, Micro>, i32>);
}

ww_si::origin! {
    /// The board's temperature sensor reads 0 at 25 °C
    pub struct BoardTemp { above: Celsius, offset: Offset::int(25) }
}

ww_si::unit! {
    /// Revolutions per minute: π/30 rad/s
    pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
}

/// Degrees per second, a gyro's rate
pub type DegreePerSecond = Per<Degree, Second>;

ww_si::scale! {
    /// One step of a gyro, 70 mdps
    pub struct Mdps70 = "", Exact(70 / 1000);
}
