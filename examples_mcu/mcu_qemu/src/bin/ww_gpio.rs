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
use ww_gpio::{BankCapabilities, Error, IoPinEnabledEvents, Level, Mode, Pull, Speed, Volt};

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();

    let mut scratch_args = [0u8; 512];
    let mut scratch_event = [0u8; 512];
    let mut scratch_err = [0u8; 32];
    let mut server = ServerState {};

    let r = api_server::stream_data_ser().port(0).pin(7).event(
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

pub struct ServerState {}

impl ServerState {
    fn port_capabilities(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> BankCapabilities<'_> {
        BankCapabilities {
            voltage: RefVec::Slice {
                slice: &[ww_si::quantity!(3.3 V f32)],
            },
            push_pull: true,
            open_drain: true,
            input: true,
            individually_configurable_pins: true,
            custom_mode: Default::default(),
            custom_pull: Default::default(),
            custom_speed: Default::default(),
        }
    }

    fn get_port_reference_voltage(&mut self, _index: [UNib32; 1]) -> Volt {
        ww_si::quantity!(3.3 V f32)
    }

    fn set_port_reference_voltage(
        &mut self,
        _index: [UNib32; 1],
        _quantity: Volt,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedReferenceVoltage)
    }

    fn port_name(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 1]) -> &'_ str {
        unimplemented!()
    }

    fn port_pin_set_output_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
        _level: Level,
    ) {
        unimplemented!()
    }

    fn port_pin_output_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
    ) -> Level {
        unimplemented!()
    }

    fn port_pin_toggle(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 2]) {
        unimplemented!()
    }

    fn port_pin_input_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
    ) -> Level {
        unimplemented!()
    }

    fn event_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    fn port_pin_set_mode(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
        _mode: Mode,
        _initial: Option<Level>,
    ) -> Result<(), Error> {
        unimplemented!()
    }

    fn port_pin_mode(&mut self, _msg_tx: &mut impl MessageSink, _index: [UNib32; 2]) -> Mode {
        unimplemented!()
    }

    fn set_port_pin_pull(&mut self, _index: [UNib32; 2], _pull: Pull) -> Result<(), Error> {
        unimplemented!()
    }

    fn get_port_pin_pull(&mut self, _index: [UNib32; 2]) -> Pull {
        unimplemented!()
    }

    fn set_port_pin_speed(&mut self, _index: [UNib32; 2], _speed: Speed) -> Result<(), Error> {
        unimplemented!()
    }

    fn get_port_pin_speed(&mut self, _index: [UNib32; 2]) -> Speed {
        unimplemented!()
    }

    fn port_pin_configure_events(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
        _enabled: IoPinEnabledEvents<'_>,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedEventType)
    }

    fn valid_indices_root_port(&mut self) -> ValidIndices<'_> {
        ValidIndices::Range(0..8)
    }

    fn valid_indices_root_port_pin(&mut self, _index: [UNib32; 1]) -> ValidIndices<'_> {
        ValidIndices::Range(0..16)
    }
}

pub mod api_server {
    wire_weaver::ww_codegen!(
        "../../examples/all_gpio_api" :: AllGpioApi for ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "./target/generated_mcu_qemu_gpio_server.rs"
    );
}
