//! The thermometer example device (`examples/thermometer_device`) on the Pico 2, over USB: one input, the RP2350's
//! own temperature sensor, and the LED on GPIO 25 as the status LED. The server, its handlers and its event loop are
//! the simulated device's; only the board differs.
//!
//! Run with `just run rp2 thermometer`, then from the PC `ww list` and `ww introspect`.
#![no_std]
#![no_main]

use core::time::Duration;

use cortex_m_rt::exception;
use defmt::{info, unwrap};
use defmt_rtt as _;
use embassy_rp::adc::{Adc, Blocking, Channel, Config as AdcConfig};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver as UsbDriver, InterruptHandler};
use panic_probe as _;
use static_cell::StaticCell;
use thermometer_api::{Gain, IndicationMode, Temperature};
use thermometer_device::{Board, Input, Reading, Thermometer};
use wire_weaver_usb_embassy::{UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};
use ww_device::Instant;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64;
const MAX_MESSAGE_LEN: usize = 1024;
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
static BOARD: StaticCell<Pico2Board> = StaticCell::new();
static THERMOMETER: StaticCell<Thermometer> = StaticCell::new();

/// The ADC's reference on the Pico 2: ADC_AVDD from the 3.3 V rail, in µV.
const VREF_UV: i32 = 3_300_000;
/// Full scale of the 12-bit ADC.
const FULL_SCALE: i32 = 4096;
/// The sensor's Vbe at 27 °C, in µV, typical (RP2350 datasheet 12.4.6).
const VBE_27C_UV: i32 = 706_000;
/// The sensor's slope, -1.721 mV per °C, typical (RP2350 datasheet 12.4.6).
const SLOPE_UV_PER_C: i32 = 1_721;

/// The temperature in 0.01 °C of a raw sensor reading: `T = 27 - (V - 0.706 V) / 1.721 mV` (RP2350 datasheet
/// 12.4.6). Typical values, no calibration: a 1 % error in the 3.3 V rail moves it by about 4 °C.
fn centi_celsius(raw: u16) -> Option<i16> {
    let uv = i32::from(raw) * VREF_UV / FULL_SCALE;
    i16::try_from(2700 - (uv - VBE_27C_UV) * 100 / SLOPE_UV_PER_C).ok()
}

/// The Pico 2 as the thermometer sees it.
///
/// The RP2350 errata (datasheet appendix D) list none for the ADC; RP2350-E9 (pad input leakage) concerns ADC inputs
/// on GPIO pins, which this board does not use.
struct Pico2Board {
    adc: Adc<'static, Blocking>,
    sensor: Channel<'static>,
    led: Output<'static>,
}

impl Board for Pico2Board {
    fn inputs(&self) -> Input {
        Input::bounded(1)
    }

    fn conversion_time(&self) -> Duration {
        // one conversion takes 2 µs at 48 MHz, rounded up with the read
        Duration::from_micros(10)
    }

    fn read(&mut self, _input: Input, _gain: Gain, _now: Instant) -> Reading {
        let centi = match self.adc.blocking_read(&mut self.sensor).ok().and_then(centi_celsius) {
            Some(centi) => centi,
            None => {
                cnt::cnt!(bad_temperature_readings: u32, warn);
                0
            }
        };
        // the internal sensor has no amplifier: it never clips
        Reading {
            temp: Temperature::from_raw(centi),
            overload: false,
        }
    }

    fn indicate(&mut self, mode: IndicationMode) {
        match mode {
            IndicationMode::AllOff => self.led.set_low(),
            IndicationMode::OnlyEssential | IndicationMode::Normal | IndicationMode::Unknown(_) => {
                self.led.set_high()
            }
        }
    }
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn thermometer_task(
    mut server: UsbServer<'static, UsbDriver<'static, USB>>,
    thermometer: &'static mut Thermometer,
) {
    thermometer_device::serve(&mut server, thermometer, core::future::pending()).await;
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    let p = embassy_rp::init(Default::default());
    let led = Output::new(p.PIN_25, Level::Low);

    let board = BOARD.init(Pico2Board {
        adc: Adc::new_blocking(p.ADC, AdcConfig::default()),
        sensor: Channel::new_temp_sensor(p.ADC_TEMP_SENSOR),
        led,
    });
    let thermometer = THERMOMETER.init(Thermometer::new(board));

    let driver = UsbDriver::new(p.USB, Irqs);
    let (usb, server) = usb_init(
        driver,
        USB_BUFFERS.init(UsbBuffers::default()),
        UsbTimings::fs_higher_speed(),
        Thermometer::link_config(),
        Thermometer::API_ID,
        |config| {
            config.product = Some("Pico 2 thermometer");
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(thermometer_task(server, thermometer)));
    info!("thermometer ready");
}

#[exception]
unsafe fn DefaultHandler(_irqn: i16) {
    cnt::cnt!(unhandled_exceptions: u32, warn);
}

#[exception]
unsafe fn HardFault(_ef: &cortex_m_rt::ExceptionFrame) -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}
