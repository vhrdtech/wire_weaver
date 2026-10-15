//! The device: [Controller] and [Motor] behind the generated `MotorDemo` server, run in its own thread on simulated
//! time over the in-process transport or UDP.
//!
//! The device's clock is simulated time, one PWM period (40 µs) per step. Each step the controller runs, the values
//! that changed are marked (`mark_changed_<name>()`, so `on_change` observers get them), and `poll_properties` sends
//! what is due. Simulated time follows real time times `sim.time_scale`; when a step can't keep up the simulation
//! falls behind instead of catching up later, `telemetry.real_time_factor` says how fast it really runs.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use motor_demo_api::{ControlMode, InjectFault, Load, Preset, Profile, SensorMode};
use motor_sim::{Faults, Motor, MotorParams, presets};
use tokio::net::UdpSocket;
use wire_weaver::prelude::*;
use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_client::in_process::{self, TokioClock};
use ww_device::{LinkConfig, MessageRx, MessageTx};
use ww_stream::observe::ObserveLimits;

use crate::controller::{
    Config, ControlValues, Controller, FocValues, PWM_HZ, Ran, TelemetryValues,
};

/// One PWM period, simulated µs.
pub const STEP_US: u64 = 1_000_000 / PWM_HZ as u64;
/// Longest message the device takes or sends.
pub const MAX_MESSAGE_LEN: usize = 1024;

/// Observe limits of the demo, in simulated time: one PWM period is the shortest period, so the 25 kHz tier can be
/// observed at its full rate (when the link keeps up), 20 kB/s per level when the host doesn't ask for a budget.
pub const LIMITS: ObserveLimits = ObserveLimits {
    min_period_us: STEP_US as u32,
    default_budget: 20_000,
    max_level_budget: 4_000_000,
    total_budget: 8_000_000,
    timestamps: true,
};

/// q current limit for each motor, A.
fn motor(preset: Preset) -> (MotorParams, f32) {
    match preset {
        Preset::IndustrialSpm => (presets::industrial_spm(), 10.0),
        Preset::Ipm => (presets::ipm(), 40.0),
        Preset::VescOutrunner | Preset::Unknown(_) => (presets::vesc_hobby_outrunner(), 60.0),
    }
}

fn sim_load(load: Load) -> motor_sim::Load {
    match load {
        Load::None | Load::Unknown(_) => motor_sim::Load::None,
        Load::Viscous { b } => motor_sim::Load::Viscous { b_nms_per_rad: b },
        Load::Fan { k } => motor_sim::Load::Fan { k_nms2_per_rad2: k },
        Load::Constant { torque } => motor_sim::Load::Constant { torque_nm: torque },
        Load::Locked => motor_sim::Load::Locked,
    }
}

/// Speed steps of [Profile::SpeedSteps], electrical rpm, half a second each.
const PROFILE_ERPM: [f32; 4] = [2_000.0, 8_000.0, -4_000.0, 12_000.0];

/// How the device starts.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub time_scale: f32,
    pub preset: Preset,
    pub load: Load,
    pub profile: Profile,
    /// Random seed of the simulator's noise.
    pub seed: u64,
}

impl Default for Options {
    /// Real time, VESC outrunner with a propeller, speed steps on.
    fn default() -> Self {
        Options {
            time_scale: 1.0,
            preset: Preset::VescOutrunner,
            load: Load::Fan { k: 2e-7 },
            profile: Profile::SpeedSteps,
            seed: 1,
        }
    }
}

/// The server: simulator, controller and the observe state.
pub struct Device {
    pub motor: Motor,
    pub ctl: Controller,
    preset: Preset,
    load: Load,
    profile: Profile,
    /// Simulated µs the profile started at.
    profile_start_us: u64,
    time_scale: f32,
    real_time_factor: f32,
    seed: u64,
    /// PWM periods since the device started, never reset: the device's clock.
    steps: u64,
    prev_foc: FocValues,
    prev_control: ControlValues,
    prev_telemetry: TelemetryValues,
    prev_real_time_factor: f32,
    pub observe: api_impl::ObserveState,
}

impl Device {
    pub fn new(opts: Options) -> Self {
        let (params, current_max) = motor(opts.preset);
        let mut d = Device {
            motor: Motor::new(params, opts.seed),
            ctl: Controller::new(Config::from_params(&params, current_max)),
            preset: opts.preset,
            load: opts.load,
            profile: opts.profile,
            profile_start_us: 0,
            time_scale: opts.time_scale,
            real_time_factor: 0.0,
            seed: opts.seed,
            steps: 0,
            prev_foc: FocValues::default(),
            prev_control: ControlValues::default(),
            prev_telemetry: TelemetryValues::default(),
            prev_real_time_factor: 0.0,
            observe: api_impl::ObserveState::new(LIMITS),
        };
        d.motor.set_load(sim_load(opts.load));
        d
    }

    /// Device time, simulated µs.
    pub fn now_us(&self) -> u64 {
        self.steps * STEP_US
    }

    pub fn time_scale(&self) -> f32 {
        self.time_scale
    }

    /// One PWM period.
    pub fn step(&mut self) {
        self.run_profile();
        let (drive, ran) = self.ctl.pwm_step(self.motor.sensors(), self.motor.truth());
        self.motor.step(&drive, 1.0 / PWM_HZ as f32);
        self.steps += 1;
        self.mark_changes(ran);
    }

    fn run_profile(&mut self) {
        if self.profile != Profile::SpeedSteps || self.ctl.calibrating() {
            return;
        }
        let half_seconds = (self.now_us() - self.profile_start_us) / 500_000;
        let erpm = PROFILE_ERPM[half_seconds as usize % PROFILE_ERPM.len()];
        if self.ctl.set.mode != ControlMode::Speed {
            self.ctl.set.mode = ControlMode::Speed;
            self.ctl.clear_fault();
            self.mark_changed_control_mode();
        }
        if self.ctl.set.erpm != erpm {
            self.ctl.set.erpm = erpm;
            self.mark_changed_control_erpm_target();
        }
    }

    fn mark_changes(&mut self, ran: Ran) {
        self.mark_foc();
        if ran.control {
            self.mark_control();
        }
        if ran.telemetry {
            self.mark_telemetry();
            if self.real_time_factor != self.prev_real_time_factor {
                self.prev_real_time_factor = self.real_time_factor;
                self.mark_changed_telemetry_real_time_factor();
            }
        }
    }

    /// Back to rest with the current motor and settings; the device clock goes on.
    fn restart(&mut self) {
        let (params, current_max) = motor(self.preset);
        let set = self.ctl.set;
        let faults = *self.motor.faults();
        self.motor = Motor::new(params, self.seed);
        self.motor.set_load(sim_load(self.load));
        *self.motor.faults_mut() = faults;
        self.ctl = Controller::new(Config::from_params(&params, current_max));
        self.ctl.set = set;
        self.profile_start_us = self.now_us();
    }

    /// A host took over the setpoints: the profile stops.
    fn manual(&mut self) {
        self.profile = Profile::Off;
    }
}

/// Getters and change marks of one tier: each value is read by its getter and marked changed when it differs from
/// the previous run of the tier.
macro_rules! tier {
    ($values:ident, $prev:ident, $mark:ident; $($name:ident: $ty:ty => $get:ident, $changed:ident;)*) => {
        impl Device {
            $(
                async fn $get(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<$ty, ()> {
                    GetResult::Value(self.ctl.$values.$name)
                }
            )*

            fn $mark(&mut self) {
                let (now, prev) = (self.ctl.$values, self.$prev);
                $(
                    if now.$name != prev.$name {
                        self.$changed();
                    }
                )*
                self.$prev = now;
            }
        }
    };
}

tier!(foc, prev_foc, mark_foc;
    i_a: f32 => get_foc_i_a, mark_changed_foc_i_a;
    i_b: f32 => get_foc_i_b, mark_changed_foc_i_b;
    i_c: f32 => get_foc_i_c, mark_changed_foc_i_c;
    i_alpha: f32 => get_foc_i_alpha, mark_changed_foc_i_alpha;
    i_beta: f32 => get_foc_i_beta, mark_changed_foc_i_beta;
    i_d: f32 => get_foc_i_d, mark_changed_foc_i_d;
    i_q: f32 => get_foc_i_q, mark_changed_foc_i_q;
    v_d: f32 => get_foc_v_d, mark_changed_foc_v_d;
    v_q: f32 => get_foc_v_q, mark_changed_foc_v_q;
    v_alpha: f32 => get_foc_v_alpha, mark_changed_foc_v_alpha;
    v_beta: f32 => get_foc_v_beta, mark_changed_foc_v_beta;
    duty_a: f32 => get_foc_duty_a, mark_changed_foc_duty_a;
    duty_b: f32 => get_foc_duty_b, mark_changed_foc_duty_b;
    duty_c: f32 => get_foc_duty_c, mark_changed_foc_duty_c;
    phase: f32 => get_foc_phase, mark_changed_foc_phase;
    phase_observer: f32 => get_foc_phase_observer, mark_changed_foc_phase_observer;
    phase_encoder: f32 => get_foc_phase_encoder, mark_changed_foc_phase_encoder;
    observer_x1: f32 => get_foc_observer_x1, mark_changed_foc_observer_x1;
    observer_x2: f32 => get_foc_observer_x2, mark_changed_foc_observer_x2;
    v_bus: f32 => get_foc_v_bus, mark_changed_foc_v_bus;
    phase_true: f32 => get_foc_phase_true, mark_changed_foc_phase_true;
);

tier!(control, prev_control, mark_control;
    erpm: f32 => get_control_erpm, mark_changed_control_erpm;
    rpm: f32 => get_control_rpm, mark_changed_control_rpm;
    id_set: f32 => get_control_id_set, mark_changed_control_id_set;
    iq_set: f32 => get_control_iq_set, mark_changed_control_iq_set;
    id_filtered: f32 => get_control_id_filtered, mark_changed_control_id_filtered;
    iq_filtered: f32 => get_control_iq_filtered, mark_changed_control_iq_filtered;
    i_motor: f32 => get_control_i_motor, mark_changed_control_i_motor;
    i_input: f32 => get_control_i_input, mark_changed_control_i_input;
    duty_now: f32 => get_control_duty_now, mark_changed_control_duty_now;
    torque: f32 => get_control_torque, mark_changed_control_torque;
    position_deg: f32 => get_control_position_deg, mark_changed_control_position_deg;
    observer_error_deg: f32 => get_control_observer_error_deg, mark_changed_control_observer_error_deg;
    rpm_true: f32 => get_control_rpm_true, mark_changed_control_rpm_true;
);

tier!(telemetry, prev_telemetry, mark_telemetry;
    time_s: f32 => get_telemetry_time_s, mark_changed_telemetry_time_s;
    amp_hours: f32 => get_telemetry_amp_hours, mark_changed_telemetry_amp_hours;
    amp_hours_charged: f32 => get_telemetry_amp_hours_charged, mark_changed_telemetry_amp_hours_charged;
    watt_hours: f32 => get_telemetry_watt_hours, mark_changed_telemetry_watt_hours;
    watt_hours_charged: f32 => get_telemetry_watt_hours_charged, mark_changed_telemetry_watt_hours_charged;
    tachometer: i32 => get_telemetry_tachometer, mark_changed_telemetry_tachometer;
    tachometer_abs: u32 => get_telemetry_tachometer_abs, mark_changed_telemetry_tachometer_abs;
    v_in: f32 => get_telemetry_v_in, mark_changed_telemetry_v_in;
    power_in: f32 => get_telemetry_power_in, mark_changed_telemetry_power_in;
    avg_motor_current: f32 => get_telemetry_avg_motor_current, mark_changed_telemetry_avg_motor_current;
    avg_input_current: f32 => get_telemetry_avg_input_current, mark_changed_telemetry_avg_input_current;
    fault: motor_demo_api::Fault => get_telemetry_fault, mark_changed_telemetry_fault;
);

/// Setpoints, sim settings and methods
impl Device {
    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }

    async fn get_telemetry_real_time_factor(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<f32, ()> {
        GetResult::Value(self.real_time_factor)
    }

    async fn set_control_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: ControlMode,
    ) -> SetResult<()> {
        if matches!(value, ControlMode::Unknown(_)) {
            return SetResult::SetError(());
        }
        self.manual();
        self.ctl.set.mode = value;
        self.ctl.clear_fault();
        self.mark_changed_control_mode();
        SetResult::Set
    }

    async fn get_control_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<ControlMode, ()> {
        GetResult::Value(self.ctl.set.mode)
    }

    async fn set_control_sensor_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: SensorMode,
    ) -> SetResult<()> {
        if matches!(value, SensorMode::Unknown(_)) {
            return SetResult::SetError(());
        }
        self.ctl.set.sensor_mode = value;
        self.mark_changed_control_sensor_mode();
        SetResult::Set
    }

    async fn get_control_sensor_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<SensorMode, ()> {
        GetResult::Value(self.ctl.set.sensor_mode)
    }

    async fn set_control_duty_target(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: f32,
    ) -> SetResult<()> {
        if !(-0.95..=0.95).contains(&value) {
            return SetResult::SetError(());
        }
        self.manual();
        self.ctl.set.duty = value;
        self.mark_changed_control_duty_target();
        SetResult::Set
    }

    async fn get_control_duty_target(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<f32, ()> {
        GetResult::Value(self.ctl.set.duty)
    }

    async fn set_control_current_target(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: f32,
    ) -> SetResult<()> {
        if !value.is_finite() {
            return SetResult::SetError(());
        }
        self.manual();
        self.ctl.set.current = value;
        self.mark_changed_control_current_target();
        SetResult::Set
    }

    async fn get_control_current_target(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<f32, ()> {
        GetResult::Value(self.ctl.set.current)
    }

    async fn set_control_erpm_target(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: f32,
    ) -> SetResult<()> {
        if !value.is_finite() {
            return SetResult::SetError(());
        }
        self.manual();
        self.ctl.set.erpm = value;
        self.mark_changed_control_erpm_target();
        SetResult::Set
    }

    async fn get_control_erpm_target(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<f32, ()> {
        GetResult::Value(self.ctl.set.erpm)
    }

    async fn set_sim_time_scale(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: f32,
    ) -> SetResult<()> {
        if !(1e-4..=1e3).contains(&value) {
            return SetResult::SetError(());
        }
        self.time_scale = value;
        SetResult::Set
    }

    async fn get_sim_time_scale(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<f32, ()> {
        GetResult::Value(self.time_scale)
    }

    async fn set_sim_load(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Load,
    ) -> SetResult<()> {
        if matches!(value, Load::Unknown(_)) {
            return SetResult::SetError(());
        }
        self.load = value;
        self.motor.set_load(sim_load(value));
        SetResult::Set
    }

    async fn get_sim_load(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<Load, ()> {
        GetResult::Value(self.load)
    }

    async fn set_sim_motor(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Preset,
    ) -> SetResult<()> {
        if matches!(value, Preset::Unknown(_)) {
            return SetResult::SetError(());
        }
        self.preset = value;
        self.restart();
        SetResult::Set
    }

    async fn get_sim_motor(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Preset, ()> {
        GetResult::Value(self.preset)
    }

    async fn set_sim_profile(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Profile,
    ) -> SetResult<()> {
        if matches!(value, Profile::Unknown(_)) {
            return SetResult::SetError(());
        }
        if value != self.profile {
            self.profile_start_us = self.now_us();
        }
        self.profile = value;
        SetResult::Set
    }

    async fn get_sim_profile(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Profile, ()> {
        GetResult::Value(self.profile)
    }

    async fn sim_inject(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        fault: InjectFault,
    ) -> RpcResult<()> {
        let f = self.motor.faults_mut();
        match fault {
            InjectFault::Clear | InjectFault::Unknown(_) => *f = Faults::NONE,
            InjectFault::CurrentOffset { phase, amps } => {
                if let Some(o) = f.sensor.current_offset_a.get_mut(phase as usize) {
                    *o = amps;
                }
            }
            InjectFault::EncoderMissedCounts { probability } => {
                f.sensor.encoder_missed_count_probability = probability.clamp(0.0, 1.0)
            }
            InjectFault::MissedPulses { probability } => {
                f.inverter.missed_pulse_probability = probability.clamp(0.0, 1.0)
            }
            InjectFault::BusVoltageOffset { volts } => f.sensor.vdc_offset_v = volts,
        }
        RpcResult::Ready(())
    }

    async fn sim_reset(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.restart();
        RpcResult::Ready(())
    }
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        motor_demo_api :: MotorDemo for super::Device,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        // debug_to_file = "../../target/motor_demo_server.rs"
    );
}

impl WireWeaverAsyncApiBackend for Device {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> wire_weaver::ww_version::FullVersion<'_> {
        motor_demo_api::MOTOR_DEMO_FULL_GID
    }
}

/// Where the device is reachable.
#[derive(Clone, Debug)]
pub enum Medium {
    /// In this process, under a path: `ClientConfig::in_process_path(path)`.
    InProcess(String),
    /// A UDP socket bound to this address: `ClientConfig::udp_addr(addr)`.
    Udp(SocketAddr),
}

/// A device running in its own thread, stopped when dropped.
pub struct DeviceThread {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    /// Address of the UDP socket, with the port the system picked for port 0.
    pub udp_addr: Option<SocketAddr>,
}

impl DeviceThread {
    /// Stop the device and wait for its thread.
    pub fn stop(mut self) -> anyhow::Result<()> {
        self.stop_and_join()
    }

    /// Run until the device fails (e.g., its socket does).
    pub fn join(mut self) -> anyhow::Result<()> {
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| anyhow::anyhow!("device thread panicked"))?,
            None => Ok(()),
        }
    }

    fn stop_and_join(&mut self) -> anyhow::Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| anyhow::anyhow!("device thread panicked"))?,
            None => Ok(()),
        }
    }
}

impl Drop for DeviceThread {
    fn drop(&mut self) {
        _ = self.stop_and_join();
    }
}

/// Start a device on `medium`.
pub fn spawn(medium: Medium, opts: Options) -> anyhow::Result<DeviceThread> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    // ww_device futures are not Send: the device gets a thread and a runtime of its own
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let socket = match &medium {
        Medium::Udp(addr) => {
            let socket = std::net::UdpSocket::bind(addr)?;
            socket.set_nonblocking(true)?;
            Some(socket)
        }
        Medium::InProcess(_) => None,
    };
    let udp_addr = socket.as_ref().map(|s| s.local_addr()).transpose()?;
    let thread = std::thread::Builder::new()
        .name("motor_demo device".into())
        .spawn(move || {
            rt.block_on(async move {
                let mut device = Device::new(opts);
                match (medium, socket) {
                    (Medium::Udp(_), Some(socket)) => {
                        serve_udp(UdpSocket::from_std(socket)?, &mut device, &stop2).await
                    }
                    (Medium::InProcess(path), _) => {
                        serve_in_process(&path, &mut device, &stop2).await
                    }
                    (Medium::Udp(_), None) => unreachable!(),
                }
            })
        })?;
    Ok(DeviceThread {
        stop,
        thread: Some(thread),
        udp_addr,
    })
}

fn link_config() -> LinkConfig<'static> {
    LinkConfig::new(
        motor_demo_api::MOTOR_DEMO_FULL_GID,
        api_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    )
}

async fn serve_in_process(
    path: &str,
    device: &mut Device,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    let (tx, rx) = in_process::device(path, MAX_MESSAGE_LEN);
    let mut scratch = [0u8; MAX_MESSAGE_LEN];
    let mut event_scratch = [0u8; MAX_MESSAGE_LEN];
    let server = ww_device::Server::new(
        link_config(),
        tx,
        rx,
        TokioClock::new(),
        &mut scratch,
        &mut event_scratch,
    );
    run(server, device, stop).await;
    Ok(())
}

/// ww_device's UDP medium on a tokio socket
struct Socket(UdpSocket);

impl ww_device::udp::DatagramSocket for Socket {
    type Addr = SocketAddr;
    type RecvError = std::io::Error;
    type SendError = std::io::Error;

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.0.recv_from(buf).await
    }

    async fn send_to(&self, datagram: &[u8], addr: SocketAddr) -> std::io::Result<()> {
        self.0.send_to(datagram, addr).await.map(|_| ())
    }
}

async fn serve_udp(
    socket: UdpSocket,
    device: &mut Device,
    stop: &AtomicBool,
) -> anyhow::Result<()> {
    use ww_device::udp::{UdpBuffers, UdpConnection, udp_server};
    let conn = UdpConnection::new(Socket(socket));
    let mut buffers = Box::new(UdpBuffers::<MAX_MESSAGE_LEN>::new());
    let server = udp_server(link_config(), &conn, TokioClock::new(), &mut buffers);
    run(server, device, stop).await;
    Ok(())
}

/// Simulated time against real time.
struct Pace {
    /// Real time and device steps at the last change of pace.
    anchor: (Instant, u64),
    scale: f32,
    /// Real time and steps at the start of the current real time factor window.
    window: (Instant, u64),
}

impl Pace {
    fn new(now: Instant, steps: u64, scale: f32) -> Self {
        Pace {
            anchor: (now, steps),
            scale,
            window: (now, steps),
        }
    }

    /// Steps the device should have done by `now`.
    fn target(&self, now: Instant) -> u64 {
        let real = now.duration_since(self.anchor.0).as_secs_f64();
        self.anchor.1 + (real * self.scale as f64 * PWM_HZ as f64) as u64
    }
}

/// Real time a tick may spend stepping before the simulation falls behind.
const TICK_BUDGET: Duration = Duration::from_millis(4);

async fn run<Tx: MessageTx, Rx: MessageRx>(
    mut server: ww_device::Server<'_, Tx, Rx, TokioClock>,
    device: &mut Device,
    stop: &AtomicBool,
) {
    let mut buf = [0u8; 512];
    let mut tick = tokio::time::interval(Duration::from_millis(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pace = Pace::new(Instant::now(), device.steps, device.time_scale);
    while !stop.load(Ordering::Relaxed) {
        tokio::select! {
            ready = server.wait() => {
                _ = server.handle(ready, device).await;
            }
            _ = tick.tick() => {
                let now = Instant::now();
                if device.time_scale != pace.scale {
                    pace = Pace::new(now, device.steps, device.time_scale);
                }
                let target = pace.target(now);
                let mut n = 0u32;
                while device.steps < target {
                    device.step();
                    if server.is_up() {
                        _ = device.poll_properties(&mut server.sink(), (), device.now_us(), &mut buf).await;
                    }
                    n += 1;
                    if n.is_multiple_of(64) && now.elapsed() > TICK_BUDGET {
                        // can't keep up: fall behind for good rather than catch up in a burst later
                        pace.anchor = (Instant::now(), device.steps);
                        break;
                    }
                }
                let window = now.duration_since(pace.window.0);
                if window >= Duration::from_millis(500) {
                    let sim = (device.steps - pace.window.1) as f64 / PWM_HZ as f64;
                    device.real_time_factor = (sim / window.as_secs_f64()) as f32;
                    pace.window = (now, device.steps);
                }
            }
        }
    }
}
