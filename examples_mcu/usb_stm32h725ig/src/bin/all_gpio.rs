#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::pac::gpio::Gpio;
use embassy_stm32::pac::{GPIOA, GPIOB, GPIOC, GPIOD, GPIOE, GPIOF, GPIOG, GPIOH, GPIOJ, GPIOK};
use embassy_stm32::time::mhz;
use embassy_stm32::{bind_interrupts, peripherals::USB_OTG_HS, usb, usb::Driver, Config};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use stm32_metapac::gpio::vals::{Idr, Moder, Odr, Ospeedr, Ot, Pupdr};
use wire_weaver::prelude::*;
use wire_weaver_usb_embassy::{usb_init, UsbBuffers, UsbServer, UsbTimings};
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
use ww_gpio::{
    AvailablePins, BankCapabilities, Error, IoPinEnabledEvents, Level, Mode, Pull, Speed, Volt,
};

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<USB_OTG_HS>;
});

const MAX_USB_PACKET_LEN: usize = 1024; // 64 for FullSpeed, 1024 for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 4096; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

#[embassy_executor::task]
async fn usb_server_task(
    mut usb_server: UsbServer<'static, Driver<'static, USB_OTG_HS>, ServerState>,
) {
    usb_server.run().await;
}

impl WireWeaverAsyncApiBackend for ServerState {
    async fn process_bytes<'a>(
        &mut self,
        msg_tx: &mut impl MessageSink,
        data: &[u8],
        scratch_args: &'a mut [u8],
        scratch_event: &'a mut [u8],
        scratch_err: &'a mut [u8],
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(data, scratch_args, scratch_event, scratch_err, msg_tx)
            .await
    }

    fn version(&self) -> FullVersion<'_> {
        all_gpio_api::ALL_GPIO_API_FULL_GID
    }
}

struct ServerState {
    bank: [Gpio; 10],
}

mod server_impl {
    wire_weaver::ww_api!(
        "../../examples/all_gpio_api/src/lib.rs" as all_gpio_api::AllGpioApi for ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = true,
        debug_to_file = "./target/generated_all_gpio_server_server.rs"
    );
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("All GPIO on STM32H725IG is starting...");

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

    let state = ServerState {
        bank: [
            GPIOA, GPIOB, GPIOC, GPIOD, GPIOE, GPIOF, GPIOG, GPIOH, GPIOJ, GPIOK,
        ],
    };

    use embassy_stm32::gpio::{Level, Output, Speed};
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
    let (usb_server, _tx) = usb_init(
        driver,
        buffers,
        state,
        UsbTimings::hs_higher_speed(),
        // UsbTimings::hs_lower_latency(),
        all_gpio_api::ALL_GPIO_API_FULL_GID,
        &server_impl::WW_API_SIGNATURE,
        ww_client_server::COMPACT_VERSION,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
        },
    );
    unwrap!(spawner.spawn(usb_server_task(usb_server)));

    info!("init done");
    loop {
        info!("loop");
        Timer::after_millis(2000).await;
        // _ = _tx.try_send(());
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    error!("Unhandled exception (IRQn = {})", irqn);
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    error!("HardFault {}", Debug2Format(ef));

    loop {}
}

impl ServerState {
    async fn port_count(&mut self, _msg_tx: &mut impl MessageSink) -> u32 {
        self.bank.len() as u32
    }

    async fn port_available(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> AvailablePins<'_> {
        AvailablePins::Range(0..16)
    }

    async fn port_capabilities(
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

    async fn port_reference_voltage(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Volt {
        ww_si::quantity!(3.3 V f32)
    }

    async fn port_set_reference_voltage(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _quantity: Volt,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedReferenceVoltage)
    }

    async fn port_set_mode(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _mode: Mode,
        _initial: Option<Level>,
    ) -> Result<(), Error> {
        defmt::todo!()
    }

    async fn port_mode(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Result<Mode, Error> {
        defmt::todo!()
    }

    async fn port_set_speed(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _pull: Speed,
    ) -> Result<(), Error> {
        defmt::todo!()
    }

    async fn port_speed(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Result<Speed, Error> {
        defmt::todo!()
    }

    async fn port_name(&mut self, _msg_tx: &mut impl MessageSink, index: [UNib32; 1]) -> &'_ str {
        match index[0].0 {
            0 => "PA",
            1 => "PB",
            2 => "PC",
            3 => "PD",
            4 => "PE",
            5 => "PF",
            6 => "PG",
            7 => "PH",
            8 => "PJ",
            9 => "PK",
            _ => "",
        }
    }

    async fn port_pin_set_output_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        index: [UNib32; 2],
        level: Level,
    ) {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let odr = self.bank[bank_idx].odr();
        let level = if level == Level::High {
            Odr::HIGH
        } else {
            Odr::LOW
        };
        odr.modify(|o| o.set_odr(pin_idx, level));
    }

    async fn port_pin_output_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        index: [UNib32; 2],
    ) -> Level {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let odr = self.bank[bank_idx].odr().read().odr(pin_idx);
        match odr {
            Odr::HIGH => Level::High,
            Odr::LOW => Level::Low,
        }
    }

    async fn port_pin_toggle(&mut self, _msg_tx: &mut impl MessageSink, index: [UNib32; 2]) {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let odr = self.bank[bank_idx].odr().read().odr(pin_idx);
        let odr = match odr {
            Odr::HIGH => Odr::LOW,
            Odr::LOW => Odr::HIGH,
        };
        self.bank[bank_idx]
            .odr()
            .modify(|o| o.set_odr(pin_idx, odr));
    }

    async fn port_pin_input_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        index: [UNib32; 2],
    ) -> Level {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let idr = self.bank[bank_idx].idr().read().idr(pin_idx);
        match idr {
            Idr::HIGH => Level::High,
            Idr::LOW => Level::Low,
        }
    }

    async fn event_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    async fn port_pin_voltage(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
    ) -> Option<Volt> {
        None
    }

    async fn port_pin_set_mode(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        index: [UNib32; 2],
        mode: Mode,
        initial: Option<Level>,
    ) -> Result<(), Error> {
        if let Some(initial) = initial {
            self.port_pin_set_output_level(_msg_tx, index, initial)
                .await;
        }
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        if mode == Mode::OpenDrainOutput {
            self.bank[bank_idx]
                .otyper()
                .modify(|o| o.set_ot(pin_idx, Ot::OPEN_DRAIN));
        } else if mode == Mode::PushPullOutput {
            self.bank[bank_idx]
                .otyper()
                .modify(|o| o.set_ot(pin_idx, Ot::PUSH_PULL));
        }
        let mode = match mode {
            Mode::PushPullOutput => Moder::OUTPUT,
            Mode::OpenDrainOutput => Moder::OUTPUT,
            Mode::Input => Moder::INPUT,
            Mode::HighZ => Moder::ANALOG,
            Mode::Analog => Moder::ANALOG,
            Mode::Custom(_) => return Err(Error::UnsupportedMode),
        };
        self.bank[bank_idx]
            .moder()
            .modify(|m| m.set_moder(pin_idx, mode));
        Ok(())
    }

    async fn port_pin_mode(&mut self, _msg_tx: &mut impl MessageSink, index: [UNib32; 2]) -> Mode {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let mode = self.bank[bank_idx].moder().read().moder(pin_idx);
        match mode {
            Moder::INPUT => Mode::Input,
            Moder::OUTPUT => {
                let ot = self.bank[bank_idx].otyper().read().ot(pin_idx);
                if ot == Ot::OPEN_DRAIN {
                    Mode::OpenDrainOutput
                } else {
                    Mode::PushPullOutput
                }
            }
            Moder::ALTERNATE => Mode::Custom(0),
            Moder::ANALOG => Mode::Analog,
        }
    }

    async fn set_port_pin_pull(&mut self, index: [UNib32; 2], pull: Pull) -> Result<(), Error> {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let pull = match pull {
            Pull::None => Pupdr::FLOATING,
            Pull::Up => Pupdr::PULL_UP,
            Pull::Down => Pupdr::PULL_DOWN,
            Pull::Custom(_) => return Err(Error::UnsupportedPull),
        };
        self.bank[bank_idx]
            .pupdr()
            .modify(|p| p.set_pupdr(pin_idx, pull));
        Ok(())
    }

    async fn get_port_pin_pull(&mut self, index: [UNib32; 2]) -> Pull {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let pull = self.bank[bank_idx].pupdr().read().pupdr(pin_idx);
        match pull {
            Pupdr::FLOATING => Pull::None,
            Pupdr::PULL_UP => Pull::Up,
            Pupdr::PULL_DOWN => Pull::Down,
            Pupdr::_RESERVED_3 => Pull::Custom(3),
        }
    }

    async fn set_port_pin_speed(&mut self, index: [UNib32; 2], speed: Speed) -> Result<(), Error> {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let speed = match speed {
            Speed::Slow => Ospeedr::LOW_SPEED,
            Speed::Medium => Ospeedr::MEDIUM_SPEED,
            Speed::Fast => Ospeedr::HIGH_SPEED,
            Speed::VeryFast => Ospeedr::VERY_HIGH_SPEED,
            Speed::Custom(_) => return Err(Error::UnsupportedSpeed),
        };
        self.bank[bank_idx]
            .ospeedr()
            .modify(|o| o.set_ospeedr(pin_idx, speed));
        Ok(())
    }

    async fn get_port_pin_speed(&mut self, index: [UNib32; 2]) -> Speed {
        let bank_idx = index[0].0 as usize;
        let pin_idx = index[1].0 as usize;
        let speed = self.bank[bank_idx].ospeedr().read().ospeedr(pin_idx);
        match speed {
            Ospeedr::LOW_SPEED => Speed::Slow,
            Ospeedr::MEDIUM_SPEED => Speed::Medium,
            Ospeedr::HIGH_SPEED => Speed::Fast,
            Ospeedr::VERY_HIGH_SPEED => Speed::VeryFast,
        }
    }

    async fn port_pin_configure_events(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 2],
        _enabled: IoPinEnabledEvents<'_>,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedEventType)
    }

    fn validate_index_port(&mut self, index: [UNib32; 1]) -> Result<(), ()> {
        let bank_idx = index[0].0 as usize;
        if bank_idx < self.bank.len() {
            Ok(())
        } else {
            Err(())
        }
    }

    fn validate_index_pin(&mut self, index: [UNib32; 2]) -> Result<(), ()> {
        let pin_idx = index[1].0 as usize;
        if pin_idx < 16 {
            Ok(())
        } else {
            Err(())
        }
    }
}
