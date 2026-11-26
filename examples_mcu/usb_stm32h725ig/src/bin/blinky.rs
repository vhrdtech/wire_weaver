//! The blinky example device (`examples/blinky_device`) on the STM32H725IG board, over USB high speed (ULPI): both
//! user LEDs (PF5 and PC6) follow its one LED. The server and its handlers are the simulated device's.
//!
//! Run with `cargo run --bin stm32h725ig-blinky`, then from the PC `ww list` and `ww call led_on`.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use blinky_device::{Blinky, Led};
use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::time::mhz;
use embassy_stm32::{
    Config, bind_interrupts,
    gpio::{Level, Output, Speed},
    peripherals::USB_OTG_HS,
    usb,
    usb::Driver,
};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver_usb_embassy::{UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<USB_OTG_HS>;
});

const MAX_USB_PACKET_LEN: usize = 1024; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 4096; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
static LED: StaticCell<BoardLeds> = StaticCell::new();
static BLINKY: StaticCell<Blinky> = StaticCell::new();

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB_OTG_HS>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, Driver<'static, USB_OTG_HS>>,
    blinky: &'static mut Blinky,
) {
    blinky_device::serve(&mut server, blinky, core::future::pending()).await;
}

/// The board's two user LEDs as the blinky device sees them: both switch together.
struct BoardLeds([Output<'static>; 2]);

impl Led for BoardLeds {
    fn set(&mut self, on: bool) {
        for led in &mut self.0 {
            if on {
                led.set_high();
            } else {
                led.set_low();
            }
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("blinky on STM32H725IG is starting...");

    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = None;
        config.rcc.csi = false;
        config.rcc.hse = Some(Hse {
            freq: mhz(24),
            mode: HseMode::Bypass,
        });
        config.rcc.hsi48 = None;
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV2,
            mul: PllMul::MUL45,
            divp: Some(PllDiv::DIV1),
            divq: Some(PllDiv::DIV4),
            divr: Some(PllDiv::DIV2),
        });
        config.rcc.pll3 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV2,
            mul: PllMul::MUL16,
            divp: Some(PllDiv::DIV2),
            divq: Some(PllDiv::DIV4), // 48MHz
            divr: Some(PllDiv::DIV4),
        });
        config.rcc.sys = Sysclk::PLL1_P; // 540MHz
        config.rcc.d1c_pre = AHBPrescaler::DIV2;
        config.rcc.ahb_pre = AHBPrescaler::DIV2;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.apb3_pre = APBPrescaler::DIV2;
        config.rcc.apb4_pre = APBPrescaler::DIV2;
        config.rcc.voltage_scale = VoltageScale::Scale0;
        config.rcc.supply_config = SupplyConfig::DirectSMPS;
        config.rcc.mux.fdcansel = mux::Fdcansel::PLL1_Q;
        config.rcc.mux.usbsel = mux::Usbsel::PLL3_Q;
        config.rcc.mux.adcsel = mux::Adcsel::PLL3_R;
        config.rcc.mux.sdmmcsel = mux::Sdmmcsel::PLL1_Q;
    }
    let p = embassy_stm32::init(config);
    info!("RCC and RAM init done");

    let led_b125 = Output::new(p.PF5, Level::Low, Speed::Low);
    let led_b135 = Output::new(p.PC6, Level::Low, Speed::Low);
    let blinky = BLINKY.init(Blinky::new(LED.init(BoardLeds([led_b125, led_b135]))));

    let _ulpi_rst_n = Output::new(p.PH3, Level::High, Speed::Low); // do not drop
    let _usb_mux_n = Output::new(p.PH5, Level::Low, Speed::Low); // do not drop

    static EP_OUT_BUF: StaticCell<[u8; EP_OUT_BUF_LEN]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUF.init([0u8; EP_OUT_BUF_LEN]);
    let config = usb::Config::default();
    let driver = Driver::new_hs_ulpi(
        p.USB_OTG_HS,
        Irqs,
        p.PA5,
        p.PC2,
        p.PC3,
        p.PC0,
        p.PA3,
        p.PB0,
        p.PB1,
        p.PB10,
        p.PB11,
        p.PB12,
        p.PB13,
        p.PB5,
        ep_out_buffer,
        config,
    );

    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::hs_higher_speed(),
        // UsbTimings::hs_lower_latency(),
        Blinky::link_config(),
        Blinky::API_ID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(ww_server_task(server, blinky)));

    info!("init done");
    loop {
        info!("loop");
        Timer::after_millis(2000).await;
        // _ = _tx.try_send(());
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
