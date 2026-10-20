use wire_weaver::prelude::*;

#[ww_trait]
trait Traits {
    ww_impl!(g1: Subgroup);
    ww_impl!(gpio[]: Gpio);
    ww_impl!(periph[]: Peripheral);
    ww_impl!(i2c: I2c);

    /// Optional item of the root level itself.
    #[optional]
    fn reset();

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

/// Optional items (API-16): a server leaves them out with `ww_codegen!(.., omit = [..])`.
#[ww_trait]
trait I2c {
    fn i2c_write(data: u8) -> u8;
    /// Clocks SCL until SDA is released.
    #[optional]
    fn i2c_wiggle_clock(count: u8) -> u8;
    /// Switchable pull-ups: both items or neither.
    /// Fallible: an omitted method with an error type gives `CallError::Unimplemented` on the client.
    #[optional(pullups)]
    fn i2c_set_pullups(enabled: bool) -> Result<(), u8>;
    #[optional(pullups)]
    property!(rw i2c_pullups: bool);
    #[optional]
    stream!(i2c_monitor: u8);
}

/// Optional trait attachments (API-54): a server leaves a whole `ww_impl!` out with `omit = [..]` or `omit_if`.
#[ww_trait]
trait Board {
    fn ping() -> u8;
    /// Pin diagnostics, a firmware feature.
    #[optional]
    ww_impl!(diag: Diag);
    /// Probes and their count: a resource array of a trait and a method, both or neither.
    #[optional(probes)]
    ww_impl!(probe[]: Gpio);
    #[optional(probes)]
    fn probe_count() -> u8;
}

#[ww_trait]
trait Diag {
    fn read(point: u16) -> u8;
    /// Served without a handler, from the server's consts trait.
    const POINTS: u16;
    ww_impl!(sub: Subgroup);
    #[optional]
    fn release_all();
}
