#![no_std]
#![no_main]
// #![feature(impl_trait_in_assoc_type)]

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
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_usb_embassy::{
    LinkConfig, UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init,
};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(
    mut server: UsbServer<'static, UsbDriver<'static, USB>>,
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
        self.process_request_bytes(data, scratch, msg_tx).await
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
        //debug_to_file = "./target/generated_blinky_server.rs"
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
    info!("USB blinky on RP235x starting...");

    let p = embassy_rp::init(Default::default());
    info!("RCC and RAM init done");

    let led = Output::new(p.PIN_25, Level::Low);

    let state = ServerState { led };

    let driver = UsbDriver::new(p.USB, Irqs);
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
        |_config| {
            // config.serial_number = Some();
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
