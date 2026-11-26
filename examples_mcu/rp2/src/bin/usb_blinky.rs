//! The blinky example device (`examples/blinky_device`) on the RP235x, over USB: the LED on GPIO 25 is its LED. The
//! server and its handlers are the simulated device's; only the LED differs.
//!
//! Run with `just run rp2 usb_blinky`, then from the PC `ww list` and `ww call led_on`.
#![no_std]
#![no_main]
// #![feature(impl_trait_in_assoc_type)]

use blinky_device::{Blinky, Led};
use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver as UsbDriver, InterruptHandler};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver_usb_embassy::{UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
static LED: StaticCell<BoardLed> = StaticCell::new();
static BLINKY: StaticCell<Blinky> = StaticCell::new();

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, UsbDriver<'static, USB>>,
    blinky: &'static mut Blinky,
) {
    blinky_device::serve(&mut server, blinky, core::future::pending()).await;
}

/// The board's LED as the blinky device sees it.
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

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("USB blinky on RP235x starting...");

    let p = embassy_rp::init(Default::default());
    info!("RCC and RAM init done");

    let led = Output::new(p.PIN_25, Level::Low);

    let blinky = BLINKY.init(Blinky::new(LED.init(BoardLed(led))));

    let driver = UsbDriver::new(p.USB, Irqs);
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::fs_higher_speed(),
        // UsbTimings::fs_lower_latency(),
        Blinky::link_config(),
        Blinky::API_ID,
        |_config| {
            // config.serial_number = Some();
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
