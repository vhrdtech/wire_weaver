#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use bbqueue::{
    BBQueue,
    nicknames::Texas,
    prod_cons::framed::{FramedConsumer, FramedProducer},
    traits::{coordination::cas::AtomicCoord, notifier::maitake::MaiNotSpsc, storage::Inline},
};
use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::{
    Config, bind_interrupts, dma,
    gpio::{Level, Output, Speed},
    mode::Async,
    peripherals,
    peripherals::USB_OTG_HS,
    time::mhz,
    usart,
    usart::{Config as UsartConfig, HalfDuplexReadback, Uart, UartRx, UartTx},
    usb,
    usb::Driver,
};
use embassy_futures::select::{Either3, select3};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver_usb_embassy::{
    LinkConfig, UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init,
};
use ww_client_server::StreamSideband;
use ww_si::Volt;
use ww_uart::{BaudRate, Capabilities, Mode, Parity, RxChunk, StopBits};

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<USB_OTG_HS>;
    UART7 => usart::InterruptHandler<peripherals::UART7>;
    UART8 => usart::InterruptHandler<peripherals::UART8>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM1 => dma::InterruptHandler<peripherals::DMA1_CH1>;
    DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
});

const MAX_USB_PACKET_LEN: usize = 512; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 4096; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

type UsbDriver = Driver<'static, USB_OTG_HS>;

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver>) {
    usb.run().await;
}

/// WireWeaver event loop: requests from the host and UART data to stream back, all in one place.
#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, UsbDriver>,
    mut state: ServerState,
    rx_consumer: [RxConsumer; 2],
) {
    loop {
        match select3(
            server.wait(),
            rx_consumer[0].wait_read(),
            rx_consumer[1].wait_read(),
        )
        .await
        {
            Either3::First(ready) => {
                if let Some(event) = server.handle(ready, &mut state).await {
                    info!("link: {}", event);
                }
            }
            Either3::Second(rg) => {
                send_received_bytes(&mut server, 0, &rg).await;
                rg.release();
            }
            Either3::Third(rg) => {
                send_received_bytes(&mut server, 1, &rg).await;
                rg.release();
            }
        }
    }
}

async fn send_received_bytes(
    server: &mut UsbServer<'static, UsbDriver>,
    index: u32,
    bytes: &[u8],
) {
    let mut out = server.sink();
    if !out.sink.is_up() {
        // nobody to send to, drop
        return;
    }
    let chunk = RxChunk {
        flags: None,
        timestamp: None,
        bytes: RefVec::new_bytes(bytes),
    };
    let r = server_impl::stream_data_ser()
        .uart(index)
        .rx_send(&chunk, &mut out)
        .await;
    if let Err(e) = r {
        error!("send_received_bytes error: {:?}", e);
    }
}

struct ServerState {
    tx_producer: [TxProducer; 2],
    // uart_baud_rate: [BaudRate; 2],
    // uart_mode: [Mode; 2],
    // uart_stop_bits: [StopBits; 2],
    // uart_parity: [Parity; 2],
    // uart_prevent_back_feed: [bool; 2],
    // uart_reference_voltage: [Volt; 2],
}

mod server_impl {
    wire_weaver::ww_codegen!(
        uart_api :: UartBridge for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
        debug_to_file = "./target/generated_uart_server.rs"
    );
}

impl WireWeaverAsyncApiBackend for ServerState {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(data, scratch, out, medium)
            .await
    }

    fn version(&self) -> FullVersion<'_> {
        uart_api::UART_BRIDGE_FULL_GID
    }
}

impl ServerState {
    pub fn valid_indices_root_uart(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..0)
    }

    async fn sideband_uart_rx(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _sideband: StreamSideband,
    ) -> Option<StreamSideband> {
        None
    }

    async fn sideband_uart_tx(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _sideband: StreamSideband,
    ) -> Option<StreamSideband> {
        None
    }

    async fn write_uart_tx(&mut self, _cx: &mut Context<'_, impl EventOut>, index: [UNib32; 1], bytes: &[u8]) {
        let index = index[0].0 as usize;
        let mut wg = self.tx_producer[index].wait_grant(bytes.len() as u16).await;
        wg.copy_from_slice(bytes);
        wg.commit(bytes.len() as u16);
    }

    async fn sideband_uart_tx_mon(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _sideband: StreamSideband,
    ) -> Option<StreamSideband> {
        None
    }

    async fn uart_capabilities(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
    ) -> RpcResult<Capabilities<'_>> {
        let cap = Capabilities {
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
        };
        Ready(cap)
    }

    async fn set_uart_baud_rate(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _baud_rate: BaudRate,
    ) -> SetResult<ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn get_uart_baud_rate(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
    ) -> GetResult<BaudRate, ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn set_uart_mode(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _mode: Mode,
    ) -> SetResult<ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn get_uart_mode(&mut self, _cx: &mut Context<'_, impl EventOut>, _index: [UNib32; 1]) -> GetResult<Mode, ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn set_uart_stop_bits(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _stop_bits: StopBits,
    ) -> SetResult<ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn get_uart_stop_bits(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
    ) -> GetResult<StopBits, ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn set_uart_parity(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _parity: Parity,
    ) -> SetResult<ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn get_uart_parity(&mut self, _cx: &mut Context<'_, impl EventOut>, _index: [UNib32; 1]) -> GetResult<Parity, ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn set_uart_prevent_back_feed(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _baud_rate: bool,
    ) -> SetResult<ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn get_uart_prevent_back_feed(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
    ) -> GetResult<bool, ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn set_uart_reference_voltage(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _voltage: Volt,
    ) -> SetResult<ww_uart::Error> {
        SetError(ww_uart::Error::UnsupportedReferenceVoltage)
    }

    async fn get_uart_reference_voltage(
        &mut self, _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
    ) -> GetResult<Volt, ww_uart::Error> {
        ww_unimplemented!()
    }

    async fn uart_set_pin_level(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _index: [UNib32; 1],
        _pin: ww_uart::Pin,
        _is_high: bool,
    ) -> RpcResult<Result<(), ww_uart::Error>> {
        ww_unimplemented!()
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
) {
    loop {
        let mut wg = rx_producer.wait_grant((RX_BUF_SIZE / 2) as u16).await;
        let r = rx.read_until_idle(&mut wg).await;
        match r {
            Ok(len) => {
                // wakes up ww_server_task
                wg.commit(len as u16);
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
    let uart7 = Uart::new(p.UART7, p.PB3, p.PB4, p.DMA1_CH0, p.DMA1_CH1, Irqs, config).unwrap();
    let (uart7_tx, uart7_rx) = uart7.split();
    static TX_BB_UART7: StaticCell<Texas<TX_BUF_SIZE, MaiNotSpsc>> = StaticCell::new();
    let tx_bb_uart7 = TX_BB_UART7.init(Texas::new());
    static RX_BB_UART7: StaticCell<Texas<RX_BUF_SIZE, MaiNotSpsc>> = StaticCell::new();
    let rx_bb_uart7 = RX_BB_UART7.init(Texas::new());

    let uart8 = Uart::new_half_duplex_on_rx(
        p.UART8,
        p.PE0,
        p.DMA1_CH2,
        p.DMA1_CH3,
        Irqs,
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
        // uart_baud_rate: [BaudRate::Baud115200; 2],
        // uart_mode: [Mode::Asynchronous; 2],
        // uart_stop_bits: [StopBits::Stop1; 2],
        // uart_parity: [Parity::None; 2],
        // uart_prevent_back_feed: [false; 2],
        // uart_reference_voltage: [ww_si::quantity!(3300 mV u16); 2],
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
    let link_config = LinkConfig::new(
        uart_api::UART_BRIDGE_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::hs_higher_speed(),
        // UsbTimings::hs_lower_latency(),
        link_config,
        server_impl::API_ID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(ww_server_task(
        server,
        state,
        [rx_bb_uart7.framed_consumer(), rx_bb_uart8.framed_consumer()],
    )));

    spawner.spawn(unwrap!(uart_tx_task(
        tx_bb_uart7.framed_consumer(),
        uart7_tx
    )));
    spawner.spawn(unwrap!(uart_rx_task(
        uart7_rx,
        rx_bb_uart7.framed_producer(),
    )));
    spawner.spawn(unwrap!(uart_tx_task(
        tx_bb_uart8.framed_consumer(),
        uart8_tx
    )));
    spawner.spawn(unwrap!(uart_rx_task(
        uart8_rx,
        rx_bb_uart8.framed_producer(),
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
