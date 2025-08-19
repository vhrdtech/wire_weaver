use wire_weaver::prelude::*;
use ww_numeric::NumericValue;

// Base
pub struct Second(pub NumericValue);
pub struct SecondU32(pub u32);
pub struct NanosecondU32(pub u32); // ?

pub struct Meter(pub NumericValue);
pub struct Kilogram(pub NumericValue);

pub struct Ampere(pub NumericValue);
pub struct Kelvin(pub NumericValue);
pub struct Mole(pub NumericValue);
pub struct Candela(pub NumericValue);

// Derived
pub struct Hertz(pub NumericValue);
pub struct Radian(pub NumericValue);
pub struct Degree(pub NumericValue);
pub struct Steradian(pub NumericValue);
pub struct Newton(pub NumericValue);
pub struct Pascal(pub NumericValue);
pub struct Joule(pub NumericValue);
pub struct Watt(pub NumericValue);
pub struct Coulomb(pub NumericValue);
pub struct Volt(pub NumericValue);
pub struct Farad(pub NumericValue);
pub struct Ohm(pub NumericValue);
pub struct Siemens(pub NumericValue);
pub struct Weber(pub NumericValue);
pub struct Tesla(pub NumericValue);
pub struct Henry(pub NumericValue);
pub struct DegreeCelsius(pub NumericValue);
pub struct Lumen(pub NumericValue);
pub struct Lux(pub NumericValue);
pub struct Becquerel(pub NumericValue);
pub struct Gray(pub NumericValue);
pub struct Sievert(pub NumericValue);
pub struct Katal(pub NumericValue);

// Related units
pub struct DeciBel(pub NumericValue);
pub struct DeciBelmW(pub NumericValue);
pub struct Litre(pub NumericValue);
pub struct ElectronVolt(pub NumericValue);

// Kinematics
pub struct Speed(pub NumericValue);
pub struct Acceleration(pub NumericValue);
pub struct Jerk(pub NumericValue);
pub struct Snap(pub NumericValue);
pub struct Yank(pub NumericValue);
pub struct AngularVelocity(pub NumericValue);
pub struct AngularAcceleration(pub NumericValue);
pub struct HertzPerSecond(pub NumericValue);
pub struct VolumetricFlow(pub NumericValue);

// Dynamic
pub struct INib8P3(i8);

impl INib8P3 {
    pub const fn new(_n: u8) -> Option<INib8P3> {
        todo!()
    }

    pub const fn minus_three() -> Self {
        INib8P3(-3 + 3)
    }
    pub const fn minus_two() -> Self {
        INib8P3(-2 + 3)
    }
    pub const fn minus_one() -> Self {
        INib8P3(-1 + 3)
    }
    pub const fn zero() -> Self {
        INib8P3(3)
    }
    pub const fn one() -> Self {
        INib8P3(1 + 3)
    }
    pub const fn two() -> Self {
        INib8P3(2 + 3)
    }
    pub const fn three() -> Self {
        INib8P3(3 + 3)
    }
    pub const fn four() -> Self {
        INib8P3(4 + 3)
    }

    pub const fn value(&self) -> i8 {
        self.0 - 3
    }
}

#[derive_shrink_wrap]
pub struct SI {
    pub second: INib8P3,
    pub meter: INib8P3,
    pub kilogram: INib8P3,
    pub ampere: INib8P3,
    #[flag]
    candela: bool,
    #[flag]
    mole: bool,
    #[flag]
    kelvin: bool,
    pub kelvin: Option<INib8P3>,
    pub mole: Option<INib8P3>,
    pub candela: Option<INib8P3>,
}

impl SerializeShrinkWrap for INib8P3 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn ser_shrink_wrap(&self, _wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        todo!()
    }
}

impl<'i> DeserializeShrinkWrap<'i> for INib8P3 {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(_rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        todo!()
    }
}
