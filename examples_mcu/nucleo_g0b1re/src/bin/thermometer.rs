//! The thermometer example device (`examples/thermometer_device`) on the Nucleo G0B1RE, over USB: one input, the
//! chip's own temperature sensor, and the user LED (PA5) as the status LED. The server, its handlers and its event
//! loop are the simulated device's; only the board differs.
//!
//! Run with `just run nucleo thermometer`, then from the PC `ww list` and `ww introspect`.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::time::Duration;

use cortex_m_rt::exception;
use defmt::{info, unwrap};
use defmt_rtt as _;
use embassy_stm32::adc::{Adc, SampleTime, Temperature as TempChannel, VrefInt};
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::{ADC1, USB};
use embassy_stm32::usb::Driver;
use embassy_stm32::{Config, bind_interrupts, usb};
use panic_probe as _;
use static_cell::StaticCell;
use thermometer_api::{Gain, IndicationMode, Temperature};
use thermometer_device::{Board, Input, Reading, Thermometer};
use wire_weaver_usb_embassy::{UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};
use ww_device::Instant;

bind_interrupts!(struct Irqs {
    USB_UCPD1_2 => usb::InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64;
const MAX_MESSAGE_LEN: usize = 1024;
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
static BOARD: StaticCell<NucleoBoard> = StaticCell::new();
static THERMOMETER: StaticCell<Thermometer> = StaticCell::new();

/// TS_CAL1: the sensor's raw reading at 30 °C, VDDA = 3.0 V (DS13560 Table 5).
const TS_CAL1: *const u16 = 0x1FFF_75A8 as *const u16;
/// TS_CAL2: the sensor's raw reading at 130 °C, VDDA = 3.0 V (DS13560 Table 5).
const TS_CAL2: *const u16 = 0x1FFF_75CA as *const u16;
/// VREFINT_CAL: the reference's raw reading at VDDA = 3.0 V (DS13560 Table 6).
const VREFINT_CAL: *const u16 = 0x1FFF_75AA as *const u16;

/// tS_temp is at least 5 µs (DS13560 Table 70); the ADC runs at 16 MHz, so 160.5 cycles are 10 µs.
const SAMPLE_TIME: SampleTime = SampleTime::CYCLES160_5;

/// The factory calibration of the temperature sensor and the reference.
#[derive(Clone, Copy)]
struct Calibration {
    ts_cal1: i32,
    ts_cal2: i32,
    vrefint_cal: i32,
}

impl Calibration {
    fn read() -> Self {
        // SAFETY: the three addresses are in the engineering bytes of system memory, always mapped and read-only
        // (DS13560 Tables 5 and 6); each is a 2-byte aligned u16.
        let (ts_cal1, ts_cal2, vrefint_cal) = unsafe {
            (
                TS_CAL1.read_volatile(),
                TS_CAL2.read_volatile(),
                VREFINT_CAL.read_volatile(),
            )
        };
        Calibration {
            ts_cal1: i32::from(ts_cal1),
            ts_cal2: i32::from(ts_cal2),
            vrefint_cal: i32::from(vrefint_cal),
        }
    }

    /// The temperature in 0.01 °C of a raw sensor reading `ts` taken while the reference read `vrefint`: the
    /// reading scaled to VDDA = 3.0 V, then the line through TS_CAL1 at 30 °C and TS_CAL2 at 130 °C (RM0444
    /// "Reading the temperature"). `None` for a reference reading of 0 or a calibration without a slope.
    fn centi_celsius(self, ts: u16, vrefint: u16) -> Option<i16> {
        let vrefint = i32::from(vrefint);
        let span = self.ts_cal2 - self.ts_cal1;
        if vrefint == 0 || span == 0 {
            return None;
        }
        let ts_at_3v = i32::from(ts) * self.vrefint_cal / vrefint;
        let centi = 3000 + (ts_at_3v - self.ts_cal1) * 10_000 / span;
        i16::try_from(centi).ok()
    }
}

/// The Nucleo G0B1RE as the thermometer sees it.
///
/// ES0548 2.6.6 (ADC offset may be out of specification, VREF+ below 3.0 V) does not apply: the Nucleo runs at
/// 3.3 V. ES0548 2.6.2 and 2.6.4 (writing ADC_CFGR1 or ADC_CFGR2 while ADEN is set resets RES and CKMODE) leave the
/// defaults this firmware uses, 12 bits and the asynchronous clock.
struct NucleoBoard {
    adc: Adc<'static, ADC1>,
    sensor: TempChannel,
    vrefint: VrefInt,
    calibration: Calibration,
    led: Output<'static>,
}

impl Board for NucleoBoard {
    fn inputs(&self) -> Input {
        Input::bounded(1)
    }

    fn conversion_time(&self) -> Duration {
        // two conversions of 10 µs and the sequencer, rounded up
        Duration::from_micros(50)
    }

    fn read(&mut self, _input: Input, _gain: Gain, _now: Instant) -> Reading {
        let vrefint = self.adc.blocking_read(&mut self.vrefint, SAMPLE_TIME);
        let ts = self.adc.blocking_read(&mut self.sensor, SAMPLE_TIME);
        let centi = match self.calibration.centi_celsius(ts, vrefint) {
            Some(centi) => centi,
            None => {
                // the calibration had no slope or the reference read 0
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
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn thermometer_task(
    mut server: UsbServer<'static, Driver<'static, USB>>,
    thermometer: &'static mut Thermometer,
) {
    thermometer_device::serve(&mut server, thermometer, core::future::pending()).await;
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    let p = embassy_stm32::init(Config::default());
    let led = Output::new(p.PA5, Level::Low, Speed::Low);

    // the sensor needs up to 120 µs after TSEN (tSTART, DS13560 Table 70): the first request comes much later
    let adc = Adc::new(p.ADC1);
    let board = BOARD.init(NucleoBoard {
        sensor: adc.enable_temperature(),
        vrefint: adc.enable_vrefint(),
        adc,
        calibration: Calibration::read(),
        led,
    });
    let thermometer = THERMOMETER.init(Thermometer::new(board));

    // --8<-- [start:usb]
    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    let (usb, server) = usb_init(
        driver,
        USB_BUFFERS.init(UsbBuffers::default()),
        UsbTimings::fs_higher_speed(),
        Thermometer::link_config(),
        Thermometer::API_ID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
            config.product = Some("Nucleo G0B1RE thermometer");
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(thermometer_task(server, thermometer)));
    // --8<-- [end:usb]
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
