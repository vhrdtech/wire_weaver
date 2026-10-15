//! API of the observed properties tests (SRV-18): `+observe` properties on the root level and on a nested one,
//! `get_set` and `value_on_changed`, sized and unsized, one property without `+observe`, and a level in a resource
//! array, which can't be observed yet.
use wire_weaver::prelude::*;

#[ww_trait]
trait Observe {
    /// get_set, sampled through the getter
    property!(ro+observe temp: f32);
    /// value_on_changed: a write marks it changed
    property!(rw+observe mode: u8);
    /// Not observable
    property!(rw plain: u8);
    /// Unsized: measured through the getter
    property!(ro+observe label: &'i str);
    /// Device changes `temp` and marks it changed
    fn set_temp(value: f32);
    ww_impl!(motor: Motor);
    ww_impl!(bank[]: Bank);
}

#[ww_trait]
trait Motor {
    property!(ro+observe speed: u32);
    property!(ro+observe current: u32);
    property!(ro+observe position: u32);
}

#[ww_trait]
trait Bank {
    property!(ro+observe level: u8);
}
