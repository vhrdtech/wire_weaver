//! The wasm slot example on B135B (SRV-85): `wasm_slot_api` over USB, with a mount slot per wasm interpreter
//! (wasefire-interpreter, wasmi: features `wasefire` and `wasmi`), each serving a module image from flash
//! (`modules/`, built by `just wasm-modules`). The host loads, reloads and unloads them, calls into them, and reads
//! the DWT cycles, stack and heap each one took. The status LED D201 breathes green, and warns after a module
//! fault. Host side: `examples/wasm_slot`.
#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

mod led;
mod slot;

use bedrock_led::{Pwm, look};
use cortex_m::peripheral::DWT;
use cortex_m_rt::exception;
use defmt::*;
use defmt_rtt as _;
use embassy_stm32::time::mhz;
use embassy_stm32::{Config, bind_interrupts, gpio::Level, gpio::Output, gpio::Speed, peripherals::USB_OTG_HS, usb, usb::Driver};
use panic_probe as _;
use static_cell::StaticCell;
use wasm_slot_api::{HeapUse, Image, Interpreter, LoadRefused, SlotCounters, Timing};
use wire_weaver::prelude::*;
use wire_weaver::ww_api::mount::MountPresence;
use wire_weaver_usb_embassy::{LinkConfig, UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_device::mount::{Module, ModuleSlot, ModuleTraits};
use ww_si::Fixed;

use crate::slot::{HEAP, HEAP_SIZE, MSG, Measured, OUT, Slot, WasefireModule, WasmiModule, paint_stack, stack_used};

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<USB_OTG_HS>;
});

const MAX_USB_PACKET_LEN: usize = 1024; // 64 for FullSpeed, 512 (Bulk) or 1024 (Interrupt) for HighSpeed
const EP_OUT_BUF_LEN: usize = MAX_USB_PACKET_LEN * wire_weaver_usb_embassy::ENDPOINTS_USED;
const MAX_MESSAGE_LEN: usize = 4096; // Maximum WireWeaver message length
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();
/// The CPU clock DWT counts, PLL1_P.
const CPU_CLOCK_HZ: u32 = 540_000_000;
/// The server task's turn for the slots and observed presences when nothing comes from the host.
const POLL_EVERY_US: u64 = 10_000;

static WASEFIRE_SLOT: StaticCell<ModuleSlot<WasefireModule, MSG, OUT>> = StaticCell::new();
static WASMI_SLOT: StaticCell<ModuleSlot<WasmiModule, MSG, OUT>> = StaticCell::new();

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB_OTG_HS>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn ww_server_task(mut server: UsbServer<'static, Driver<'static, USB_OTG_HS>>, mut state: ServerState) {
    let mut buf = [0u8; 256];
    loop {
        let now = ww_device::Instant::from_micros(embassy_time::Instant::now().as_micros());
        let ready = server
            .wait_until(Some(now + core::time::Duration::from_micros(POLL_EVERY_US)))
            .await;
        if !ready.is_due() {
            if let Some(event) = server.handle(ready, &mut state).await {
                info!("{:?}", event);
            }
            continue;
        }
        if !server.is_up() {
            continue;
        }
        let out = &mut server.sink();
        state.wasefire.slot.poll(out).await;
        state.wasmi.slot.poll(out).await;
        state.presence_changed();
        let now_us = embassy_time::Instant::now().as_micros();
        if state.poll_properties(out, (), now_us, &mut buf).await.is_err() {
            cnt::cnt!(observe_send_failed: u32, warn);
        }
    }
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
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> FullVersion<'_> {
        wasm_slot_api::WASM_SLOT_FULL_GID
    }

    fn global_into_mount<'b>(&self, request: &[u8], buf: &'b mut [u8]) -> Option<&'b [u8]> {
        ModuleTraits::<2>::new()
            .slot(&server_impl::WASEFIRE_API_PATH, self.wasefire.slot)
            .slot(&server_impl::WASMI_API_PATH, self.wasmi.slot)
            .request_into_mount(request, server_impl::has_global_trait, buf)
    }
}

struct ServerState {
    wasefire: Slot<WasefireModule>,
    wasmi: Slot<WasmiModule>,
    observe: server_impl::ObserveState,
}

mod server_impl {
    wire_weaver::ww_codegen!(
        wasm_slot_api :: WasmSlot for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs"
    );
}

/// The module `image` names, loaded by `interpreter`, with the cycles and stack the load took.
fn load_image<M: Module>(
    slot: &mut Slot<M>,
    image: Image,
    new: impl FnOnce(&'static [u8]) -> Result<M, LoadRefused>,
) -> Result<(), LoadRefused> {
    let wasm = match image {
        Image::Calc => slot::CALC,
        Image::CalcEvolved => slot::CALC_EVOLVED,
        Image::Unknown(_) => return Err(LoadRefused::NoImage),
    };
    // two modules don't fit the heap at once (wasmi's take ~130 KiB): the old one goes first
    drop(slot.slot.unload());
    let top = paint_stack();
    let start = DWT::cycle_count();
    let module = new(wasm);
    slot.measured.instantiate = DWT::cycle_count().wrapping_sub(start);
    slot.measured.instantiate_stack = stack_used(top);
    let Ok(module) = module else {
        cnt::cnt!(module_instantiate_refused: u32, warn);
        return module.map(|_| ());
    };
    let top = paint_stack();
    let start = DWT::cycle_count();
    let loaded = slot.slot.load(module).map(drop);
    slot.measured.load = DWT::cycle_count().wrapping_sub(start);
    slot.measured.load_stack = stack_used(top);
    loaded.map_err(|_| {
        cnt::cnt!(module_model_refused: u32, warn);
        LoadRefused::Model
    })
}

impl ServerState {
    fn presence_changed(&mut self) {
        if self.wasefire.slot.take_changed() {
            self.mark_changed_wasefire_presence();
        }
        if self.wasmi.slot.take_changed() {
            self.mark_changed_wasmi_presence();
        }
        let faults = self.wasefire.slot.counters().faults + self.wasmi.slot.counters().faults;
        if faults > 0 {
            led::LEDS.set(0, look::WARN.mode);
            led::LEDS.set_colour(0, look::WARN.colour);
        }
    }

    async fn get_wasefire_presence(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MountPresence<'_>, CommonError<'_>> {
        Value(self.wasefire.slot.presence())
    }

    async fn wasefire_request(
        &mut self,
        cx: &mut Context<'_, impl EventOut>,
        request: MountRequest<'_, '_>,
    ) -> MountAnswer {
        let answer = forward(&mut self.wasefire, cx, &request).await;
        self.presence_changed();
        answer
    }

    async fn get_wasmi_presence(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MountPresence<'_>, CommonError<'_>> {
        Value(self.wasmi.slot.presence())
    }

    async fn wasmi_request(
        &mut self,
        cx: &mut Context<'_, impl EventOut>,
        request: MountRequest<'_, '_>,
    ) -> MountAnswer {
        let answer = forward(&mut self.wasmi, cx, &request).await;
        self.presence_changed();
        answer
    }

    async fn load(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        interpreter: Interpreter,
        image: Image,
    ) -> RpcResult<Result<(), wire_weaver::Error<'_, LoadRefused>>> {
        let loaded = match interpreter {
            Interpreter::Wasefire => load_image(&mut self.wasefire, image, new_wasefire),
            Interpreter::Wasmi => load_image(&mut self.wasmi, image, new_wasmi),
            Interpreter::Unknown(_) => Err(LoadRefused::NotBuilt),
        };
        self.presence_changed();
        if loaded.is_ok() {
            led::LEDS.set(0, look::OK.mode);
            led::LEDS.set_colour(0, look::OK.colour);
        }
        Ready(loaded.map_err(wire_weaver::Error::Api))
    }

    async fn unload(&mut self, _cx: &mut Context<'_, impl EventOut>, interpreter: Interpreter) -> RpcResult<()> {
        match interpreter {
            Interpreter::Wasefire => drop(self.wasefire.slot.unload()),
            Interpreter::Wasmi => drop(self.wasmi.slot.unload()),
            Interpreter::Unknown(_) => {}
        }
        self.presence_changed();
        Ready(())
    }

    async fn counters(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        interpreter: Interpreter,
    ) -> RpcResult<Result<SlotCounters, CommonError<'_>>> {
        let c = match interpreter {
            Interpreter::Wasefire => self.wasefire.slot.counters(),
            Interpreter::Wasmi => self.wasmi.slot.counters(),
            Interpreter::Unknown(_) => return Ready(Err(CommonError::Unsupported)),
        };
        Ready(Ok(SlotCounters {
            faults: Fixed::new(c.faults),
            refused: Fixed::new(c.refused.saturating_add(c.lost_answers)),
            dropped: Fixed::new(
                c.malformed
                    .saturating_add(c.stray)
                    .saturating_add(c.dropped)
                    .saturating_add(c.unsent),
            ),
        }))
    }

    async fn timing(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        interpreter: Interpreter,
    ) -> RpcResult<Result<Timing, CommonError<'_>>> {
        let m = match interpreter {
            Interpreter::Wasefire => self.wasefire.measured,
            Interpreter::Wasmi => self.wasmi.measured,
            Interpreter::Unknown(_) => return Ready(Err(CommonError::Unsupported)),
        };
        Ready(Ok(Timing {
            instantiate: Fixed::new(m.instantiate),
            instantiate_stack: Fixed::new(m.instantiate_stack),
            load: Fixed::new(m.load),
            load_stack: Fixed::new(m.load_stack),
            forward: Fixed::new(m.forward),
            process: Fixed::new(m.process),
            forward_stack: Fixed::new(m.forward_stack),
            cpu_clock: Fixed::new(CPU_CLOCK_HZ),
        }))
    }

    async fn heap(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<HeapUse> {
        Ready(HeapUse {
            used: Fixed::new(HEAP.used() as u32),
            peak: Fixed::new(HEAP.peak() as u32),
            size: Fixed::new(HEAP_SIZE as u32),
        })
    }

    async fn reset_heap_peak(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        HEAP.reset_peak();
        Ready(())
    }

    fn observe_state(&mut self) -> &mut server_impl::ObserveState {
        &mut self.observe
    }
}

/// Forwards `request` into `slot`, measuring the forward's cycles and stack and the module's `process`.
async fn forward<M: Module + TimedProcess>(
    slot: &mut Slot<M>,
    cx: &mut Context<'_, impl EventOut>,
    request: &MountRequest<'_, '_>,
) -> MountAnswer {
    let top = paint_stack();
    let start = DWT::cycle_count();
    let answer = slot.slot.forward(cx, request).await;
    let forward = DWT::cycle_count().wrapping_sub(start);
    let forward_stack = stack_used(top);
    if answer == MountAnswer::Forwarded {
        slot.measured.forward = forward;
        slot.measured.forward_stack = forward_stack;
        if let Some(process) = slot.slot.module().map(|m| m.process_cycles()) {
            slot.measured.process = process;
        }
    }
    answer
}

/// The cycles of a module's last `process`.
trait TimedProcess {
    fn process_cycles(&self) -> u32;
}

#[cfg(any(feature = "wasefire", feature = "wasmi"))]
impl<M> TimedProcess for slot::Timed<M> {
    fn process_cycles(&self) -> u32 {
        self.process
    }
}

#[cfg(not(all(feature = "wasefire", feature = "wasmi")))]
impl TimedProcess for slot::NotBuilt {
    fn process_cycles(&self) -> u32 {
        match *self {}
    }
}

#[cfg(feature = "wasefire")]
fn new_wasefire(wasm: &'static [u8]) -> Result<WasefireModule, LoadRefused> {
    let module = ww_wasm::wasefire::WasefireModule::new(wasm, slot::WASEFIRE_MEMORY).map_err(|e| {
        warn!("wasefire-interpreter refused the module: {}", e);
        LoadRefused::Instantiate
    })?;
    Ok(slot::Timed { module, process: 0 })
}

#[cfg(not(feature = "wasefire"))]
fn new_wasefire(_: &'static [u8]) -> Result<WasefireModule, LoadRefused> {
    Err(LoadRefused::NotBuilt)
}

#[cfg(feature = "wasmi")]
fn new_wasmi(wasm: &'static [u8]) -> Result<WasmiModule, LoadRefused> {
    let limits = ww_wasm::wasmi::Limits {
        events: OUT,
        ..Default::default()
    };
    let module = ww_wasm::wasmi::WasmiModule::new(wasm, limits).map_err(|e| {
        warn!("wasmi refused the module: {}", e);
        LoadRefused::Instantiate
    })?;
    Ok(slot::Timed { module, process: 0 })
}

#[cfg(not(feature = "wasmi"))]
fn new_wasmi(_: &'static [u8]) -> Result<WasmiModule, LoadRefused> {
    Err(LoadRefused::NotBuilt)
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    info!("wasm slot on B135B is starting...");

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
        config.rcc.mux.usbsel = mux::Usbsel::PLL3_Q;
    }
    let p = embassy_stm32::init(config);
    // the USB OTG driver moves packets through its FIFO, no DMA: the D-cache is safe
    if let Some(mut cp) = cortex_m::Peripherals::take() {
        cp.SCB.enable_icache();
        cp.SCB.enable_dcache(&mut cp.CPUID);
        cp.DCB.enable_trace();
        cp.DWT.enable_cycle_counter();
    }
    HEAP.init();
    info!("RCC, caches and heap done");

    // the status LED first: lit at full brightness while its pins float low
    {
        use embassy_stm32::gpio::OutputType;
        use embassy_stm32::time::khz;
        use embassy_stm32::timer::complementary_pwm::{ComplementaryPwm, ComplementaryPwmPin};
        use embassy_stm32::timer::low_level::CountingMode;
        use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};

        let rg = SimplePwm::new(
            p.TIM3,
            Some(PwmPin::new(p.PC6, OutputType::PushPull)),
            Some(PwmPin::new(p.PC7, OutputType::PushPull)),
            None,
            None,
            khz(1),
            CountingMode::EdgeAlignedUp,
        );
        let channels = rg.split();
        let (mut green, mut red) = (channels.ch1, channels.ch2);
        green.enable();
        red.enable();
        let blue = ComplementaryPwm::new(
            p.TIM8,
            None,
            Some(ComplementaryPwmPin::new(p.PH13, OutputType::PushPull)),
            None,
            None,
            None,
            None,
            None,
            None,
            khz(1),
            CountingMode::EdgeAlignedUp,
        );
        let (red, green, blue) = (Pwm::active_low(red), Pwm::active_low(green), led::Blue::new(blue));
        spawner.spawn(unwrap!(led::led_task(red, green, blue)));
    }

    let state = ServerState {
        wasefire: Slot {
            slot: ModuleSlot::init(WASEFIRE_SLOT.uninit()),
            measured: Measured::default(),
        },
        wasmi: Slot {
            slot: ModuleSlot::init(WASMI_SLOT.uninit()),
            measured: Measured::default(),
        },
        observe: server_impl::ObserveState::new(Default::default()),
    };

    // held for good: dropped, the pins float and the ULPI PHY goes back into reset
    core::mem::forget(Output::new(p.PH3, Level::High, Speed::Low)); // ULPI_RST_N
    core::mem::forget(Output::new(p.PH5, Level::Low, Speed::Low)); // USB_MUX_N

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
        wasm_slot_api::WASM_SLOT_FULL_GID,
        wasm_slot_api::WASM_SLOT_WIRE_VERSION,
        server_impl::api_hash(),
        server_impl::image_facts(),
        ww_client_server::COMPACT_VERSION,
    );
    let (usb, server) = usb_init(
        driver,
        buffers,
        UsbTimings::hs_higher_speed(),
        link_config,
        server_impl::API_ID,
        |config| {
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(ww_server_task(server, state)));
    info!("init done");
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    cnt::cnt!(unhandled_exceptions: u32, warn);
    error!("Unhandled exception (IRQn = {})", irqn);
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    error!("HardFault {}", Debug2Format(ef));

    loop {}
}
