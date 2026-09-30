#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::{
    Config, bind_interrupts,
    gpio::{Level, Output, Speed},
    peripherals::USB_OTG_FS,
    usb,
    usb::Driver,
};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver_usb_embassy::{
    LinkConfig, UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init,
};

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<USB_OTG_FS>;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, Driver<'static, USB_OTG_FS>>,
    mut state: ServerState,
) {
    // Nothing else to wait for: use the prepared loop. See the uart example for a custom one.
    server.run(&mut state).await;
}

impl WireWeaverAsyncApiBackend for ServerState {
    async fn process_bytes<'a>(
        &mut self,
        msg_tx: &mut impl MessageSink,
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(data, scratch, msg_tx)
            .await
    }

    fn version(&self) -> FullVersion<'_> {
        blinky_api::BLINKY_API_FULL_GID
    }
}

struct ServerState {
    led: Output<'static>,
}

mod server_impl {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
        debug_to_file = "./target/generated_blinky_server.rs"
    );
}

impl ServerState {
    async fn led_on(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
        self.led.set_high();
        Ready(())
    }

    async fn led_off(&mut self, _msg_tx: &mut impl MessageSink) -> RpcResult<()> {
        self.led.set_low();
        Ready(())
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("blinky on Nucleo H743ZI2 is starting...");

    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;
        config.rcc.hsi = Some(HSIPrescaler::DIV1);
        config.rcc.csi = true;
        config.rcc.hsi48 = Some(Hsi48Config {
            sync_from_usb: true,
        }); // needed for USB
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSI,
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL50,
            fracn: None,
            divp: Some(PllDiv::DIV2),
            divq: None,
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P; // 400 Mhz
        config.rcc.ahb_pre = AHBPrescaler::DIV2; // 200 Mhz
        config.rcc.apb1_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb2_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb3_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.apb4_pre = APBPrescaler::DIV2; // 100 Mhz
        config.rcc.voltage_scale = VoltageScale::Scale1;
        config.rcc.mux.usbsel = mux::Usbsel::HSI48;
    }
    let p = embassy_stm32::init(config);
    info!("RCC and RAM init done");

    let led = Output::new(p.PE1, Level::Low, Speed::Low);
    let state = ServerState { led };

    static EP_OUT_BUF: StaticCell<[u8; EP_OUT_BUF_LEN]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUF.init([0u8; EP_OUT_BUF_LEN]);
    let config = usb::Config::default();
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, config);
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let link_config = LinkConfig::new(
        blinky_api::BLINKY_API_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    );
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::fs_higher_speed(),
        // UsbTimings::fs_lower_latency(),
        link_config,
        server_impl::API_ID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
            // optionally set config.manufacturer, config.product, self_powered and max_power
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(ww_server_task(server, state)));

    info!("init done");
    loop {
        info!("loop");
        Timer::after_millis(2000).await;
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
