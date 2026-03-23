use wire_weaver::prelude::*;

#[ww_trait]
trait Traits {
    ww_impl!(g1: Subgroup);
    ww_impl!(gpio[]: Gpio);
    ww_impl!(periph[]: Peripheral);

    // TODO: print proper error when implementing same trait twice?
    // trait from crates
    // trait addressing
}

#[ww_trait]
trait Subgroup {
    fn m1();
}

#[ww_trait]
trait Gpio {
    fn set_high();
}

#[ww_trait]
trait Peripheral {
    ww_impl!(channel[]: Channel);
}

#[ww_trait]
trait Channel {
    property!(rw gain: f32);
    fn run();
}
