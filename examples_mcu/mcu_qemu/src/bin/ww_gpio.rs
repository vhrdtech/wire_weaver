#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use ww_client_server::stream::StreamHead;
use ww_gpio::IoPinEvent;
use ww_gpio::{
    BankCapabilities, BankConfig, GpioError, IoPinEnabledEvents, Level, Mode,
    PinCapabilities, Pull, Speed,
};
use ww_stream::tx::Counted;

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();

    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut server = ServerState {
        pin_events: Counted::new(),
        stream_sessions: 0,
    };

    let r = api_server::stream_data_ser().port(0).pin(7).event(
        &StreamHead::at(0),
        &IoPinEvent::RisingEdge,
        &mut scratch,
    );
    writeln!(stdout, "{r:02x?}").unwrap();

    let event = [1u8, 2, 3];
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    let r = server.process_request_bytes(&event, &mut scratch, &mut out, ());
    writeln!(stdout, "{r:?}").unwrap();

    // exit QEMU
    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}

struct DummyTx;
impl wire_weaver::BlockingMessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

pub struct ServerState {
    pin_events: Counted,
    stream_sessions: u32,
}

impl ServerState {
    fn port_capabilities(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 1],
    ) -> RpcResult<BankCapabilities<'_>> {
        let cap = BankCapabilities {
            io_levels: MaxLen::new_unchecked(RefVec::Slice {
                slice: &const { [ww_si::Volt::from_f32(3.3)] },
            }),
            io_level_min: None,
            io_level_max: None,
            push_pull: true,
            open_drain: true,
            input: true,
            individually_configurable_pins: true,
            custom_mode: Default::default(),
            custom_pull: Default::default(),
            custom_speed: Default::default(),
        };
        Ready(cap)
    }

    fn port_configure(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 1],
        config: BankConfig,
    ) -> RpcResult<Result<BankConfig, GpioError<'_>>> {
        // fixed 3.3 V, no level translators
        if config.io_level.is_some() {
            return Ready(Err(CommonError::Unsupported.into()));
        }
        Ready(Ok(BankConfig { io_level: None }))
    }

    fn port_name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 1],
    ) -> RpcResult<&'_ str> {
        ww_unimplemented!()
    }

    fn port_pin_set_output_level(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
        _level: Level,
    ) -> RpcResult<()> {
        ww_unimplemented!()
    }

    fn port_pin_output_level(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> RpcResult<Level> {
        ww_unimplemented!()
    }

    fn port_pin_toggle(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> RpcResult<()> {
        ww_unimplemented!()
    }

    fn port_pin_input_level(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> RpcResult<Level> {
        ww_unimplemented!()
    }

    /// This board reports no pin events (`PinCapabilities::events` is false), so the stream never opens.
    fn port_pin_event(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
        _enabled: IoPinEnabledEvents<'_>,
    ) -> RpcResult<Result<(), GpioError<'_>>> {
        Ready(Err(CommonError::Unsupported.into()))
    }

    fn close_port_pin_event(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) {
    }

    /// One state for every pin: the stream never opens on this board.
    fn stream_port_pin_event(&mut self, _index: [UNib32; 2]) -> &mut Counted {
        &mut self.pin_events
    }

    fn new_stream_session(&mut self) -> u32 {
        self.stream_sessions = self.stream_sessions.wrapping_add(1);
        self.stream_sessions
    }

    fn port_pin_set_mode(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
        _mode: Mode,
        _initial: Option<Level>,
    ) -> RpcResult<Result<(), GpioError<'_>>> {
        ww_unimplemented!()
    }

    fn port_pin_mode(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> RpcResult<Mode> {
        ww_unimplemented!()
    }

    fn set_port_pin_pull(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
        _pull: Pull,
    ) -> SetResult<GpioError<'_>> {
        ww_unimplemented!()
    }

    fn get_port_pin_pull(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> GetResult<Pull, GpioError<'_>> {
        ww_unimplemented!()
    }

    fn set_port_pin_speed(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
        _speed: Speed,
    ) -> SetResult<GpioError<'_>> {
        ww_unimplemented!()
    }

    fn get_port_pin_speed(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> GetResult<Speed, GpioError<'_>> {
        ww_unimplemented!()
    }

    fn port_pin_capabilities(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: [UNib32; 2],
    ) -> RpcResult<PinCapabilities<'_>> {
        ww_unimplemented!()
    }

    fn valid_indices_root_port(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..8)
    }

    fn valid_indices_root_port_pin(&mut self, _index: [UNib32; 1]) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..16)
    }
}

pub mod api_server {
    wire_weaver::ww_codegen!(
        all_gpio_api :: AllGpioApi for super::ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        debug_to_file = "./target/generated_mcu_qemu_gpio_server.rs"
    );
}
