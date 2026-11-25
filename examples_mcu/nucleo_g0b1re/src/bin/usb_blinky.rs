//! The blinky example device (`examples/blinky_device`) on the Nucleo G0B1RE, over USB: the user LED (PA5) is its
//! LED. The server and its handlers are the simulated device's; only the LED differs.
//!
//! The Nucleo-64 has no USB connector for the STM32: wire a USB breakout to PA11 (D-), PA12 (D+) and GND. Run with
//! `just run nucleo usb_blinky`, then from the PC `ww list` and `ww call led_on`.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use blinky_device::{Blinky, Led};
use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::{
    Config, bind_interrupts,
    gpio::{Level, Output, Speed},
    peripherals::USB,
    usb,
    usb::Driver,
};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver_usb_embassy::{UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};

bind_interrupts!(struct Irqs {
    USB_UCPD1_2 => usb::InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
// ASCII label fits into 126 bytes (USB string descriptor limit), longer ones are truncated
static API_ID_BUF: StaticCell<[u8; 126]> = StaticCell::new();
static LED: StaticCell<BoardLed> = StaticCell::new();
static BLINKY: StaticCell<Blinky> = StaticCell::new();

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, Driver<'static, USB>>,
    blinky: &'static mut Blinky,
) {
    blinky_device::serve(&mut server, blinky, core::future::pending()).await;
}

// --8<-- [start:led]
/// The board's user LED as the blinky device sees it.
struct BoardLed(Output<'static>);

impl Led for BoardLed {
    fn set(&mut self, on: bool) {
        if on {
            self.0.set_high();
        } else {
            self.0.set_low();
        }
    }
}
// --8<-- [end:led]

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("blinky on STM32G0B1 starting...");

    let p = embassy_stm32::init(Config::default());
    info!("RCC and RAM init done");

    #[cfg(feature = "b129a_cannify")]
    let led = Output::new(p.PB14, Level::Low, Speed::Low);
    #[cfg(feature = "nucleo_g0b1re")]
    let led = Output::new(p.PA5, Level::Low, Speed::Low);

    // --8<-- [start:device]
    let blinky = BLINKY.init(Blinky::new(LED.init(BoardLed(led))));
    // --8<-- [end:device]

    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    // API id without a label is a constant: pass Blinky::API_ID directly.
    // User label would normally be loaded from flash, it is shown in device listings without opening the device.
    let label = "Nucleo on the desk";
    let api_id =
        wire_weaver::api_id::with_label(Blinky::API_ID, label, API_ID_BUF.init([0; 126]));
    info!("API id: {}", api_id);
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::fs_higher_speed(),
        // UsbTimings::fs_lower_latency(),
        Blinky::link_config(),
        api_id,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
            config.product = Some("Nucleo G0B1RE blinky");
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(ww_server_task(server, blinky)));

    info!("init done");
    loop {
        info!("loop");
        Timer::after_millis(2000).await;
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    cnt::cnt!(unhandled_exceptions: u32, warn);
    error!("Unhandled exception (IRQn = {})", irqn);
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    error!("HardFault {}", Debug2Format(ef));

    loop {}
}
