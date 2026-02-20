#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

mod init;

use api::LedState;
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
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_usb_embassy::{UsbBuffers, UsbServer, UsbTimings, usb_init};
use ww_client_server::{StreamSidebandCommand, StreamSidebandEvent};

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<USB_OTG_FS>;
});

const MAX_USB_PACKET_LEN: usize = 64; // 64 for FullSpeed, 1024 for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 1024; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

#[embassy_executor::task]
async fn usb_server_task(
    mut usb_server: UsbServer<'static, Driver<'static, USB_OTG_FS>, ServerState>,
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

    async fn send_updates(
        &mut self,
        msg_tx: &mut impl MessageSink,
        scratch_value: &mut [u8],
        scratch_event: &mut [u8],
    ) {
        let message = server_impl::stream_data_ser().usart_rx(
            &RefVec::new_bytes(&[0, 1, 2, 3, 4]),
            scratch_value,
            scratch_event,
        );
        _ = msg_tx.send(message.unwrap()).await;
    }

    fn version(&self) -> FullVersion<'_> {
        api::DEVICE_API_ROOT_FULL_GID
    }
}

struct ServerState {
    led: Output<'static>,
}

mod server_impl {
    wire_weaver::ww_api!(
        "../../api/src/lib.rs" as api::DeviceApiRoot for ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = true,
        debug_to_file = "./target/generated_no_std_server.rs" // uncomment if you want to see the resulting AST and generated code
    );
}

impl ServerState {
    async fn led_on(&mut self, _msg_tx: &mut impl MessageSink) {
        self.led.set_high(); 
    }

    async fn led_off(&mut self, _msg_tx: &mut impl MessageSink) {
        self.led.set_low();
    }
    
    async fn set_led_state(&mut self, _msg_tx: &mut impl MessageSink, state: LedState) {
        match state {
            LedState::Off => self.led.set_low(),
            LedState::On => self.led.set_high(),
            LedState::Blinking => {}
        }
    }

    async fn usart_rx_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }

    async fn usart_tx_write(&mut self, data: &[u8]) {
        info!("tx: {:?}", data);
    }

    async fn usart_tx_sideband(
        &mut self,
        _msg_tx: &mut impl MessageSink,
        _cmd: StreamSidebandCommand,
    ) -> Option<StreamSidebandEvent> {
        None
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("cannify_micro_g0b1cetxn starting...");

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
    init::reset_bkp_domain();
    info!("RCC and RAM init done");

    let led = Output::new(p.PE1, Level::Low, Speed::Low);
    let state = ServerState { led };

    static EP_OUT_BUF: StaticCell<[u8; EP_OUT_BUF_LEN]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUF.init([0u8; EP_OUT_BUF_LEN]);
    let config = usb::Config::default();
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, config);
    let buffers = USB_BUFFERS.init(UsbBuffers::default());
    let (usb_server, _call_send_updates) = usb_init(
        driver,
        buffers,
        state,
        UsbTimings::default_fs(),
        api::DEVICE_API_ROOT_FULL_GID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
            // optionally set config.manufacturer, config.product, self_powered and max_power
        },
    );
    unwrap!(spawner.spawn(usb_server_task(usb_server)));

    info!("init done");
    loop {
        info!("loop");
        Timer::after_millis(2000).await;
        // _ = call_send_updates.try_send(());
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
