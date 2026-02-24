#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::{
    bind_interrupts,
    gpio::{Level, Output, Speed},
    peripherals::USB,
    usb,
    usb::Driver,
    Config,
};
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_usb_embassy::{usb_init, UsbBuffers, UsbServer, UsbTimings};

bind_interrupts!(struct Irqs {
    USB_UCPD1_2 => usb::InterruptHandler<USB>;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 1024 for HighSpeed
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

#[embassy_executor::task]
async fn usb_server_task(mut usb_server: UsbServer<'static, Driver<'static, USB>, ServerState>) {
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
        blinky_api::BLINKY_API_FULL_GID
    }
}

struct ServerState {
    led: Output<'static>,
}

mod server_impl {
    wire_weaver::ww_api!(
        "../../examples/blinky_api/src/lib.rs" as blinky_api::BlinkyApi for ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = true,
        debug_to_file = "./target/generated_no_std_server.rs"
    );
}

impl ServerState {
    async fn led_on(&mut self, _msg_tx: &mut impl MessageSink) {
        self.led.set_high();
    }

    async fn led_off(&mut self, _msg_tx: &mut impl MessageSink) {
        self.led.set_low();
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("blinky on STM32G0B1 starting...");

    let p = embassy_stm32::init(Config::default());
    info!("RCC and RAM init done");

    let led = Output::new(p.PB14, Level::Low, Speed::Low);
    let state = ServerState { led };

    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let (usb_server, tx) = usb_init(
        driver,
        buffers,
        state,
        UsbTimings::fs_higher_speed(),
        // UsbTimings::fs_lower_latency(),
        blinky_api::BLINKY_API_FULL_GID,
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
        _ = tx.try_send(());
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
