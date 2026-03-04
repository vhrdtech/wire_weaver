#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use bbqueue::{
    nicknames::Texas,
    prod_cons::framed::{FramedConsumer, FramedProducer},
    traits::{coordination::cas::AtomicCoord, notifier::maitake::MaiNotSpsc, storage::Inline},
    BBQueue,
};
use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::{
    bind_interrupts, dma,
    gpio::{Level, Output, Speed},
    mode::Async,
    peripherals,
    peripherals::USB_OTG_HS,
    time::mhz,
    usart,
    usart::{Config as UsartConfig, HalfDuplexReadback, Uart, UartRx, UartTx},
    usb,
    usb::Driver,
    Config,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Sender;
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver_usb_embassy::{usb_init, UsbBuffers, UsbServer, UsbTimings};
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};
use ww_si::Volt;
use ww_uart::{BaudRate, Capabilities, Mode, Parity, RxChunk, StopBits};

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<USB_OTG_HS>;
    UART7 => usart::InterruptHandler<peripherals::UART7>;
    UART8 => usart::InterruptHandler<peripherals::UART8>;
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

struct ServerState {
    tx_producer: [TxProducer; 2],
    rx_consumer: [RxConsumer; 2],
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

    async fn send_updates(
        &mut self,
        sink: &mut impl MessageSink,
        scratch_value: &mut [u8],
        scratch_event: &mut [u8],
    ) {
        self.send_received_bytes(0, scratch_value, scratch_event, sink)
            .await;
        self.send_received_bytes(1, scratch_value, scratch_event, sink)
            .await;
    }

    fn version(&self) -> FullVersion<'_> {
        uart_api::UART_BRIDGE_FULL_GID
    }
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

    async fn send_received_bytes(
        &mut self,
        index: usize,
        scratch_value: &mut [u8],
        scratch_event: &mut [u8],
        sink: &mut impl MessageSink,
    ) {
        if let Ok(rg) = self.rx_consumer[index].read() {
            let stream_data_event = server_impl::stream_data_ser().uart(index as u32).rx(
                &RxChunk {
                    flags: None,
                    timestamp: None,
                    bytes: RefVec::new_bytes(&rg),
                },
                scratch_value,
                scratch_event,
            );
            rg.release();
            if let Ok(stream_data_event) = stream_data_event {
                let r = sink.send(stream_data_event).await;
            }
        }
    }

    async fn tx_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _index: [UNib32; 1],
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    async fn tx_write(&mut self, index: [UNib32; 1], bytes: &[u8]) {
        let index = index[0].0 as usize;
        let mut wg = self.tx_producer[index].wait_grant(bytes.len() as u16).await;
        wg.copy_from_slice(bytes);
        wg.commit(bytes.len() as u16);
    }

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

const TX_BUF_SIZE: usize = 512;
type TxProducer = FramedProducer<&'static BBQueue<Inline<TX_BUF_SIZE>, AtomicCoord, MaiNotSpsc>>;
type TxConsumer = FramedConsumer<&'static BBQueue<Inline<TX_BUF_SIZE>, AtomicCoord, MaiNotSpsc>>;

const RX_BUF_SIZE: usize = 512;
type RxProducer = FramedProducer<&'static BBQueue<Inline<RX_BUF_SIZE>, AtomicCoord, MaiNotSpsc>>;
type RxConsumer = FramedConsumer<&'static BBQueue<Inline<RX_BUF_SIZE>, AtomicCoord, MaiNotSpsc>>;

#[embassy_executor::task(pool_size = 2)]
async fn uart_tx_task(tx_consumer: TxConsumer, mut tx: UartTx<'static, Async>) {
    loop {
        let rg = tx_consumer.wait_read().await;
        let r = tx.write(&rg).await;
        rg.release();
        match r {
            Ok(_) => {}
            Err(e) => {
                error!("uart write error: {:?}", e);
            }
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn uart_rx_task(
    mut rx: UartRx<'static, Async>,
    rx_producer: RxProducer,
    send_updates_tx: Sender<'static, CriticalSectionRawMutex, (), 1>,
) {
    loop {
        let mut wg = rx_producer.wait_grant((RX_BUF_SIZE / 2) as u16).await;
        let r = rx.read_until_idle(&mut wg).await;
        match r {
            Ok(len) => {
                wg.commit(len as u16);
                _ = send_updates_tx.send(());
            }
            Err(e) => {
                wg.commit(0);
                error!("uart read error: {:?}", e);
            }
        }
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
    let uart7 = Uart::new(p.UART7, p.PB3, p.PB4, Irqs, p.DMA1_CH0, p.DMA1_CH1, config).unwrap();
    let (uart7_tx, uart7_rx) = uart7.split();
    static TX_BB_UART7: StaticCell<Texas<TX_BUF_SIZE, MaiNotSpsc>> = StaticCell::new();
    let tx_bb_uart7 = TX_BB_UART7.init(Texas::new());
    static RX_BB_UART7: StaticCell<Texas<RX_BUF_SIZE, MaiNotSpsc>> = StaticCell::new();
    let rx_bb_uart7 = RX_BB_UART7.init(Texas::new());

    let uart8 = Uart::new_half_duplex_on_rx(
        p.UART8,
        p.PE0,
        Irqs,
        p.DMA1_CH2,
        p.DMA1_CH3,
        config,
        HalfDuplexReadback::NoReadback,
    )
    .unwrap();
    let (uart8_tx, uart8_rx) = uart8.split();
    static TX_BB_UART8: StaticCell<Texas<TX_BUF_SIZE, MaiNotSpsc>> = StaticCell::new();
    let tx_bb_uart8 = TX_BB_UART8.init(Texas::new());
    static RX_BB_UART8: StaticCell<Texas<RX_BUF_SIZE, MaiNotSpsc>> = StaticCell::new();
    let rx_bb_uart8 = RX_BB_UART8.init(Texas::new());

    let state = ServerState {
        tx_producer: [tx_bb_uart7.framed_producer(), tx_bb_uart8.framed_producer()],
        rx_consumer: [rx_bb_uart7.framed_consumer(), rx_bb_uart8.framed_consumer()],
        uart_baud_rate: [BaudRate::Baud115200; 2],
        uart_mode: [Mode::Asynchronous; 2],
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
    let (usb_server, send_updates_tx) = usb_init(
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

    unwrap!(spawner.spawn(uart_tx_task(tx_bb_uart7.framed_consumer(), uart7_tx)));
    unwrap!(spawner.spawn(uart_rx_task(
        uart7_rx,
        rx_bb_uart7.framed_producer(),
        send_updates_tx.clone(),
    )));
    unwrap!(spawner.spawn(uart_tx_task(tx_bb_uart8.framed_consumer(), uart8_tx)));
    unwrap!(spawner.spawn(uart_rx_task(
        uart8_rx,
        rx_bb_uart8.framed_producer(),
        send_updates_tx.clone(),
    )));

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
