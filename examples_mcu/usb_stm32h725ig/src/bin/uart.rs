#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::time::mhz;
use embassy_stm32::{
    bind_interrupts, dma,
    gpio::{Level, Output, Speed},
    peripherals,
    peripherals::USB_OTG_HS,
    usart,
    usart::{Config as UsartConfig, Uart},
    usb,
    usb::Driver,
    Config,
};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver_usb_embassy::{usb_init, UsbBuffers, UsbServer, UsbTimings};
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
use ww_si::Volt;
use ww_uart::{BaudRate, Capabilities, Mode, Parity, StopBits};

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<USB_OTG_HS>;
    UART7 => usart::InterruptHandler<peripherals::UART7>;
    // DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    // DMA1_STREAM1 => dma::InterruptHandler<peripherals::DMA1_CH1>;
});

const MAX_USB_PACKET_LEN: usize = 512; // 64 for FullSpeed, 512 (Bulk) 1024 (Irq) for HighSpeed
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
        uart_api::UART_BRIDGE_FULL_GID
    }
}

struct ServerState {
    uart_baud_rate: [BaudRate; 2],
    uart_mode: [Mode; 2],
    uart_stop_bits: [StopBits; 2],
    uart_parity: [Parity; 2],
    uart_prevent_back_feed: [bool; 2],
    uart_reference_voltage: [Volt; 2],
}

mod server_impl {
    wire_weaver::ww_api!(
        "../../examples/uart_api/src/lib.rs" as uart_api::UartBridge for ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=value_on_changed",
        introspect = true,
        debug_to_file = "./target/generated_uart_server.rs"
    );
}

impl ServerState {
    fn validate_index_uart(&self, index: [UNib32; 1]) -> Result<(), ()> {
        if index[0].0 > 1 {
            return Err(());
        }
        Ok(())
    }

    async fn rx_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    async fn tx_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    async fn tx_write(&mut self, index: [UNib32; 1], bytes: &[u8]) {}

    async fn tx_mon_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    async fn uart_capabilities(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
    ) -> Capabilities<'_> {
        Capabilities {
            min_baud_rate: 0,
            max_baud_rate: 0,
            voltages: RefVec::Slice {
                slice: &[ww_si::quantity!(3.3 V f32)],
            },
            rx_timestamps: false,
            hw_flow_control: false,
            sw_flow_control: false,
            high_z_mode: false,
            test_mode: false,
            back_feed_detector: false,
        }
    }

    async fn set_uart_baud_rate(
        &mut self,
        index: [UNib32; 1],
        baud_rate: BaudRate,
    ) -> Result<(), ww_uart::Error> {
        Ok(())
    }

    async fn set_uart_mode(
        &mut self,
        index: [UNib32; 1],
        mode: Mode,
    ) -> Result<(), ww_uart::Error> {
        Ok(())
    }

    async fn set_uart_stop_bits(
        &mut self,
        index: [UNib32; 1],
        stop_bits: StopBits,
    ) -> Result<(), ww_uart::Error> {
        Ok(())
    }

    async fn set_uart_parity(
        &mut self,
        index: [UNib32; 1],
        parity: Parity,
    ) -> Result<(), ww_uart::Error> {
        Ok(())
    }

    async fn set_uart_prevent_back_feed(
        &mut self,
        index: [UNib32; 1],
        baud_rate: bool,
    ) -> Result<(), ww_uart::Error> {
        Err(ww_uart::Error::Unsupported)
    }

    async fn set_uart_reference_voltage(
        &mut self,
        _index: [UNib32; 1],
        _voltage: Volt,
    ) -> Result<(), ww_uart::Error> {
        Err(ww_uart::Error::UnsupportedReferenceVoltage)
    }

    async fn uart_set_pin_level(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _pin: ww_uart::Pin,
        _is_high: bool,
    ) -> Result<(), ww_uart::Error> {
        Err(ww_uart::Error::Unsupported)
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("UART bridge on STM32H725IG is starting...");

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
        // config.rcc.mux.fdcansel = mux::Fdcansel::PLL1_Q;
        config.rcc.mux.usbsel = mux::Usbsel::PLL3_Q;
        // config.rcc.mux.adcsel = mux::Adcsel::PLL3_R;
        // config.rcc.mux.sdmmcsel = mux::Sdmmcsel::PLL1_Q;
    }
    let p = embassy_stm32::init(config);
    info!("RCC and RAM init done");

    // let led_b125 = Output::new(p.PF5, Level::Low, Speed::Low);
    // let led_b135 = Output::new(p.PC6, Level::Low, Speed::Low);

    let config = UsartConfig::default();
    let mut uart7 = Uart::new(p.UART7, p.PB3, p.PB4, Irqs, p.DMA1_CH0, p.DMA1_CH1, config).unwrap();
    // let mut uart7 = Uart::new_blocking(p.UART7, p.PB3, p.PB4, config).unwrap();
    let state = ServerState {
        uart_baud_rate: [BaudRate::Baud115200; 2],
        uart_mode: [Mode::HighZ; 2],
        uart_stop_bits: [StopBits::Stop1; 2],
        uart_parity: [Parity::None; 2],
        uart_prevent_back_feed: [false; 2],
        uart_reference_voltage: [ww_si::quantity!(3300 mV u16); 2],
    };

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
        uart_api::UART_BRIDGE_FULL_GID,
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
