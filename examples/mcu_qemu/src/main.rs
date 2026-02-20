#![no_main]
#![no_std]

mod ww;

extern crate panic_semihosting;

use cortex_m_rt::entry;
use cortex_m_semihosting::{hio, debug};
use core::fmt::Write;
use wire_weaver::prelude::BufReader;
use ww_gpio::IoPinEvent;

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();

    let mut scratch_args = [0u8; 512];
    let mut scratch_event = [0u8; 512];
    let mut scratch_err = [0u8; 32];
    let mut server = ww::ServerState {};

    let r = ww::api_server::stream_data_ser().bank_a().pin(7).event(&IoPinEvent::RisingEdge, &mut scratch_args, &mut scratch_event);
    writeln!(stdout, "{r:02x?}").unwrap();

    let event = [1u8, 2, 3];
    let r = server.process_request_bytes(&event, &mut scratch_args, &mut scratch_event, &mut scratch_err);
    writeln!(stdout, "{r:?}").unwrap();

    // exit QEMU
    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}