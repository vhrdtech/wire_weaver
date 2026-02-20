#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use core::future::Future;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
use ww_gpio::IoPinEvent;
use ww_gpio::{
    AvailablePins, BankCapabilities, Error, IoPinEnabledEvents, Level, Mode, Pull, Speed, Volt,
};

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();

    let mut scratch_args = [0u8; 512];
    let mut scratch_event = [0u8; 512];
    let mut scratch_err = [0u8; 32];
    let mut server = ServerState {};

    let r = api_server::stream_data_ser().bank_a().pin(7).event(
        &IoPinEvent::RisingEdge,
        &mut scratch_args,
        &mut scratch_event,
    );
    writeln!(stdout, "{r:02x?}").unwrap();

    let event = [1u8, 2, 3];
    let r = server.process_request_bytes(
        &event,
        &mut scratch_args,
        &mut scratch_event,
        &mut scratch_err,
        &mut DummyTx {},
    );
    writeln!(stdout, "{r:?}").unwrap();

    // exit QEMU
    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}

struct DummyTx;
impl wire_weaver::MessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> impl Future<Output = Result<(), ()>> {
        core::future::ready(Ok(()))
    }
}

#[ww_trait]
pub trait McuGpio {
    ww_impl!(bank_a: "../../../ww_stdlib/ww_gpio/src/lib.rs" as ww_gpio::GpioBank);
}

pub struct ServerState {}

impl ServerState {
    fn bank_a_available(&mut self, _msg_tx: &mut impl MessageSink) -> AvailablePins<'_> {
        todo!()
    }

    fn bank_a_capabilities(&mut self, _msg_tx: &mut impl MessageSink) -> BankCapabilities<'_> {
        todo!()
    }

    fn bank_a_reference_voltage(&mut self, _msg_tx: &mut impl MessageSink) -> Volt {
        todo!()
    }

    fn bank_a_set_reference_voltage(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _quantity: Volt,
    ) -> Result<(), Error> {
        todo!()
    }

    fn bank_a_set_mode(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _mode: Mode,
        _initial: Option<Level>,
    ) -> Result<(), Error> {
        todo!()
    }

    fn bank_a_mode(&mut self, _msg_tx: &mut impl MessageSink) -> Result<Mode, Error> {
        todo!()
    }

    fn bank_a_set_speed(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _pull: Speed,
    ) -> Result<(), Error> {
        todo!()
    }

    fn bank_a_speed(&mut self, _msg_tx: &mut impl MessageSink) -> Result<Speed, Error> {
        todo!()
    }

    fn bank_a_name(&mut self, _msg_tx: &mut impl MessageSink) -> &'_ str {
        todo!()
    }

    fn bank_a_pin_set_high(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 1]) {}

    fn bank_a_pin_set_low(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 1]) {}

    fn bank_a_pin_set_output_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _level: Level,
    ) {
    }

    fn bank_a_pin_output_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Level {
        todo!()
    }

    fn bank_a_pin_toggle(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 1]) {}

    fn bank_a_pin_input_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Level {
        todo!()
    }

    fn event_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn bank_a_pin_voltage(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Option<Volt> {
        None
    }

    fn bank_a_pin_set_mode(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _mode: Mode,
        _initial: Option<Level>,
    ) -> Result<(), Error> {
        todo!()
    }

    fn bank_a_pin_mode(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 1]) -> Mode {
        todo!()
    }

    fn set_bank_a_pin_pull(&mut self, _index: [UNib32; 1], _pull: Pull) -> Result<(), Error> {
        todo!()
    }

    fn get_bank_a_pin_pull(&mut self, _index: [UNib32; 1]) -> Pull {
        todo!()
    }

    fn set_bank_a_pin_speed(&mut self, _index: [UNib32; 1], _pull: Speed) -> Result<(), Error> {
        todo!()
    }

    fn get_bank_a_pin_speed(&mut self, _index: [UNib32; 1]) -> Speed {
        todo!()
    }

    fn bank_a_pin_configure_events(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _enabled: IoPinEnabledEvents,
    ) -> Result<(), Error> {
        todo!()
    }

    fn validate_index_pin(&mut self, _index: [UNib32; 1]) -> Result<(), ()> {
        Ok(())
    }
}

pub mod api_server {
    use super::*;

    ww_api!(
        "src/bin/ww_gpio.rs" as mcu_gpio::McuGpio for ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "../../target/generated_mcu_qemu_gpio_server.rs"
    );
}
