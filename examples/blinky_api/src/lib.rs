#![no_std]

use wire_weaver::prelude::*;

#[ww_trait]
pub trait DeviceApiRoot {
    fn led_on();
    fn led_off();
    fn set_led_state(state: LedState);
    // fn do_work(query: Query<'i>) -> u16;
    // property!(ro button_pressed: bool);
    // stream_up!(core_temperature: f32);

    stream!(usart_rx: [u8]);
    sink!(usart_tx: [u8]);
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
pub enum LedState {
    Off,
    On,
    Blinking,
}

/// Example type that uses borrowed data on `no_std`,
/// but in addition have an owned version with the name `QueryOwned` which uses String, when `std` feature is active.
#[derive_shrink_wrap]
#[owned = "std"]
pub struct Query<'i> {
    input: &'i str,
}
