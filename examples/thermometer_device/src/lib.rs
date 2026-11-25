//! The thermometer example device ([thermometer_api]): the server, its handlers and its event loop, written once.
//!
//! The same code runs in three places. A simulated device on a PC serves it over WebSocket or UDP with
//! [SimBoard]; the firmware of each board serves it over USB with the board's own [Board]; tests serve it in
//! process. What differs between them is only the [Board] and the transport given to [serve].
//!
//! ```text
//!               thermometer_api (the trait and types)
//!                          |
//!               thermometer_device (this crate: Thermometer, serve)
//!              /           |              \
//!   SimBoard over WS    Nucleo G0B1RE     Pico 2
//!   (examples/thermometer) (examples_mcu/...)
//! ```
#![cfg_attr(not(feature = "std"), no_std)]

mod sim;

use core::future::Future;
use core::pin::pin;
use core::time::Duration;

use embassy_futures::select::{Either, select};
use thermometer_api::{Command, Gain, IndicationMode, Mode, Rate, RateError, Sample, Temperature};
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use ww_device::{Clock, Instant, LinkConfig, MessageRx, MessageTx, Server};
use ww_si::prefix::One;
use ww_si::units::{Hertz, Item, Ratio, Sample as SampleUnit};
use ww_si::{Fixed, IndexFrom1, Range, Seq};

pub use sim::SimBoard;

/// An input of the board, 1 to 4 as printed on it.
pub type Input = IndexFrom1<Item, u8, Range<1, 4>>;

/// Inputs a board can have at most.
pub const INPUTS_MAX: usize = 4;

/// Rate after reset.
const RATE_AT_RESET: Rate = Rate::bounded(10);
/// Gain after reset.
const GAIN_AT_RESET: Gain = Gain::bounded(4);
/// The answer to a `channel[]` index past [INPUTS_MAX].
const NO_SUCH_INPUT: CommonError<'static> = CommonError::Refused {
    reason: MaxLen::new_unchecked("no such input"),
};
/// Room for one observed value's event, for `poll_properties`.
const POLL_BUF_LEN: usize = 64;

// --8<-- [start:board]
/// What a board gives the thermometer: its temperature inputs and its status LED.
pub trait Board {
    /// The highest input number: `channel[]` serves inputs 1 to this one.
    fn inputs(&self) -> Input;

    /// How long a triggered conversion takes, from `Command::Trigger` to its sample.
    fn conversion_time(&self) -> Duration;

    /// Reads `input` at `gain` now.
    fn read(&mut self, input: Input, gain: Gain, now: Instant) -> Reading;

    /// Shows `mode` on the status LED.
    fn indicate(&mut self, mode: IndicationMode);
}

/// One reading of an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub temp: Temperature,
    /// The amplifier clipped at this gain; the thermometer lowers the gain.
    pub overload: bool,
}
// --8<-- [end:board]

/// Events the thermometer dropped or refused, for a firmware's counters or a test.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counters {
    /// Samples taken while no host was connected, or that the link refused.
    pub samples_not_sent: u32,
    /// Continuous samples skipped because the loop came too late for them.
    pub samples_missed: u32,
    /// `Command::Trigger` outside `Mode::Triggered`, or while a conversion ran.
    pub triggers_ignored: u32,
    /// Commands with a variant this firmware doesn't know.
    pub commands_unknown: u32,
}

/// The thermometer: the server state that the generated code dispatches to.
pub struct Thermometer {
    board: &'static mut dyn Board,
    observe: api_impl::ObserveState,
    mode: Mode,
    rate: Rate,
    gain: Gain,
    seq: Seq<SampleUnit, u16>,
    enabled: [bool; INPUTS_MAX],
    last: [Option<Sample>; INPUTS_MAX],
    indication: IndicationMode,
    /// When the next continuous sample is due.
    next_sample: Option<Instant>,
    /// When the running triggered conversion ends.
    conversion_done: Option<Instant>,
    /// Now on the server's clock, set by [serve] before each handler runs.
    now: Instant,
    counters: Counters,
    poll_buf: [u8; POLL_BUF_LEN],
}

impl Thermometer {
    /// A thermometer on `board`, idle, at 10 Hz and gain 4, every input enabled.
    pub fn new(board: &'static mut dyn Board) -> Self {
        board.indicate(IndicationMode::Normal);
        Thermometer {
            board,
            observe: api_impl::ObserveState::new(Default::default()),
            mode: Mode::Idle,
            rate: RATE_AT_RESET,
            gain: GAIN_AT_RESET,
            seq: Seq::new(0),
            enabled: [true; INPUTS_MAX],
            last: [None; INPUTS_MAX],
            indication: IndicationMode::Normal,
            next_sample: None,
            conversion_done: None,
            now: Instant::from_micros(0),
            counters: Counters::default(),
            poll_buf: [0; POLL_BUF_LEN],
        }
    }

    /// What was dropped or refused so far.
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// The link configuration a transport needs to serve this API.
    pub fn link_config() -> LinkConfig<'static> {
        LinkConfig::new(
            thermometer_api::THERMO_FULL_GID,
            thermometer_api::THERMO_WIRE_VERSION,
            api_impl::api_hash(),
            api_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        )
    }

    /// The API id a USB device names itself by.
    pub const API_ID: &'static str = api_impl::API_ID;

    /// The next time the thermometer has work of its own: a sample, the end of a conversion or an observer due.
    fn next_wake(&mut self) -> Option<Instant> {
        let observe = self
            .observe
            .next_wake(self.now.as_micros())
            .map(Instant::from_micros);
        [self.next_sample, self.conversion_done, observe]
            .into_iter()
            .flatten()
            .min()
    }

    fn period(&self) -> Duration {
        Duration::from_micros(1_000_000 / u64::from(self.rate.get()))
    }

    /// Reads `input`, lowers the gain on an overload, and keeps the sample as the input's last.
    fn sample(&mut self, input: Input) -> Sample {
        let reading = self.board.read(input, self.gain, self.now);
        if reading.overload
            && let Ok(lower) = Gain::try_new(self.gain.get() / 2)
        {
            self.gain = lower;
            self.mark_changed_gain();
        }
        let sample = Sample {
            seq: self.seq,
            input,
            temp: reading.temp,
        };
        self.seq = self.seq.next();
        self.last[usize::from(input.get() - 1)] = Some(sample);
        sample
    }

    /// The inputs that are enabled, in order.
    fn enabled_inputs(&self) -> impl Iterator<Item = Input> + use<> {
        let enabled = self.enabled;
        (1..=self.board.inputs().get())
            .filter(move |n| enabled[usize::from(n - 1)])
            .filter_map(|n| Input::try_new(n).ok())
    }

    /// Does what is due at [Self::now]: a continuous sample of every enabled input, the end of a triggered
    /// conversion, and the observers. Sends to `out` while `up`.
    async fn tick(&mut self, out: &mut impl EventOut, up: bool) {
        let now = self.now;
        if self.conversion_done.is_some_and(|at| at <= now) {
            self.conversion_done = None;
            if let Some(input) = self.enabled_inputs().next() {
                let sample = self.sample(input);
                self.send(&sample, out, up).await;
            }
        }
        if let Some(at) = self.next_sample.filter(|at| *at <= now) {
            for input in self.enabled_inputs() {
                let sample = self.sample(input);
                self.send(&sample, out, up).await;
            }
            let period = self.period();
            let mut next = at + period;
            while next <= now {
                self.counters.samples_missed = self.counters.samples_missed.saturating_add(1);
                next = next + period;
            }
            self.next_sample = Some(next);
        }
        let mut buf = self.poll_buf;
        _ = self
            .poll_properties(out, (), now.as_micros(), &mut buf)
            .await;
    }

    async fn send(&mut self, sample: &Sample, out: &mut impl EventOut, up: bool) {
        let sent = up
            && api_impl::stream_data_ser()
                .samples_send(sample, out)
                .await
                .is_ok();
        if !sent {
            self.counters.samples_not_sent = self.counters.samples_not_sent.saturating_add(1);
        }
    }

    async fn set_rate(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        rate: Fixed<Hertz, One, u16, Range<1, 1000>>,
    ) -> RpcResult<Result<(), wire_weaver::Error<'_, RateError>>> {
        if self.conversion_done.is_some() {
            return Ready(Err(RateError::Busy.into()));
        }
        self.rate = rate;
        if self.next_sample.is_some() {
            self.next_sample = Some(self.now + self.period());
        }
        Ready(Ok(()))
    }

    async fn read_once(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Sample> {
        Ready(self.sample(Input::bounded(1)))
    }

    async fn set_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Mode,
    ) -> SetResult<CommonError<'_>> {
        self.next_sample = match value {
            Mode::Continuous => Some(self.next_sample.unwrap_or(self.now)),
            Mode::Idle | Mode::Triggered | Mode::Unknown(_) => None,
        };
        self.mode = value;
        SetResult::Set
    }

    async fn get_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Mode, CommonError<'_>> {
        GetResult::Value(self.mode)
    }

    async fn set_gain(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Fixed<Ratio, One, u8, Range<1, 16>>,
    ) -> SetResult<CommonError<'_>> {
        // observers get the new gain: the generated code marks it changed after `Set`
        self.gain = value;
        SetResult::Set
    }

    async fn get_gain(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Fixed<Ratio, One, u8, Range<1, 16>>, CommonError<'_>> {
        GetResult::Value(self.gain)
    }

    async fn samples(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }

    async fn close_samples(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

    async fn sideband_samples(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }

    async fn sideband_commands(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }

    async fn write_commands(&mut self, _cx: &mut Context<'_, impl EventOut>, value: Command) {
        match value {
            Command::Trigger if self.mode == Mode::Triggered && self.conversion_done.is_none() => {
                self.conversion_done = Some(self.now + self.board.conversion_time());
            }
            Command::Trigger => {
                self.counters.triggers_ignored = self.counters.triggers_ignored.saturating_add(1);
            }
            Command::Mark { seq } => self.seq = seq,
            Command::Unknown(_) => {
                self.counters.commands_unknown = self.counters.commands_unknown.saturating_add(1);
            }
        }
    }

    fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..u32::from(self.board.inputs().get()))
    }

    /// The input at a `channel[]` index the generated code has checked against [Self::valid_indices_root_channel].
    fn input(index_chain: [ww_client_server::ResourceIndex; 1]) -> Option<Input> {
        let index = u8::try_from(index_chain[0].get().0).ok()?;
        Input::try_new(index.checked_add(1)?).ok()
    }

    async fn set_channel_enabled(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
        value: bool,
    ) -> SetResult<CommonError<'_>> {
        let Some(input) = Self::input(index_chain) else {
            return SetResult::SetError(NO_SUCH_INPUT);
        };
        self.enabled[usize::from(input.get() - 1)] = value;
        SetResult::Set
    }

    async fn get_channel_enabled(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> GetResult<bool, CommonError<'_>> {
        match Self::input(index_chain) {
            Some(input) => GetResult::Value(self.enabled[usize::from(input.get() - 1)]),
            None => GetResult::GetError(NO_SUCH_INPUT),
        }
    }

    async fn channel_last(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ww_client_server::ResourceIndex; 1],
    ) -> RpcResult<Result<Sample, CommonError<'_>>> {
        let Some(input) = Self::input(index_chain) else {
            return Ready(Err(NO_SUCH_INPUT));
        };
        let last = self.last[usize::from(input.get() - 1)];
        Ready(Ok(last.unwrap_or_else(|| self.sample(input))))
    }

    async fn set_indication_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: IndicationMode,
    ) -> SetResult<CommonError<'_>> {
        self.indication = value;
        self.board.indicate(value);
        SetResult::Set
    }

    async fn get_indication_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<IndicationMode, CommonError<'_>> {
        GetResult::Value(self.indication)
    }

    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        thermometer_api :: Thermo for super::Thermometer,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
    );
}

impl WireWeaverAsyncApiBackend for Thermometer {
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
        thermometer_api::THERMO_FULL_GID
    }
}

// --8<-- [start:serve]
/// Serves `thermometer` on `server` until `stop` resolves: the one event loop of the simulated device, the firmware
/// and the tests. Between requests it sleeps until its own next deadline: a sample, the end of a triggered
/// conversion, or an observer due. A firmware that never stops passes [core::future::pending].
pub async fn serve<Tx: MessageTx, Rx: MessageRx, C: Clock>(
    server: &mut Server<'_, Tx, Rx, C>,
    thermometer: &mut Thermometer,
    stop: impl Future<Output = ()>,
) {
    let mut stop = pin!(stop);
    loop {
        thermometer.now = server.now();
        let at = thermometer.next_wake();
        let ready = match select(server.wait_until(at), &mut stop).await {
            Either::First(ready) => ready,
            Either::Second(()) => return,
        };
        thermometer.now = server.now();
        if ready.is_due() {
            let up = server.is_up();
            thermometer.tick(&mut server.sink(), up).await;
        } else {
            _ = server.handle(ready, thermometer).await;
        }
    }
}
// --8<-- [end:serve]
