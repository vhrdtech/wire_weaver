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

/// `#[ww_trait]` makes a Rust trait for rustdoc (`wire_weaver::api_doc`), these fail to compile if its shape changes.
#[cfg(test)]
#[allow(dead_code)]
mod doc_trait {
    use super::*;
    use wire_weaver::api_doc::Property;

    fn subgroup<S: Subgroup>(_: fn() -> S) {}
    fn gpio_array<G: Gpio>(_: fn(UNib32) -> G) {}
    fn peripheral_array<P: Peripheral>(_: fn(UNib32) -> P) {}
    fn channel_array<C: Channel>(_: fn(UNib32) -> C) {}

    fn traits<T: Traits>() {
        subgroup(T::g1);
        gpio_array(T::gpio);
        peripheral_array(T::periph);
    }

    fn peripheral<P: Peripheral>() {
        channel_array(P::channel);
    }

    fn channel<C: Channel>() {
        let _: fn() -> Property<f32> = C::gain;
        let _: fn() = C::run;
    }
}
