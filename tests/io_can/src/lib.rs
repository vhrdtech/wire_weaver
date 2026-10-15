//! `ww_can_bus` end to end: two mock servers on `ww_device::Server` through `tests_common::start_device` (a full
//! node with every optional item, and a minimal node that leaves all of them out), the generated client through the
//! real host event loop.
//!
//! The mock controller runs in internal loopback: every frame sent on `tx` (or from a dedicated TX buffer) goes
//! through the acceptance filters and comes back on `rx`, and is reported on `tx_events`.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tests_common::TestDevice;
    use wire_weaver::prelude::*;
    use wire_weaver_client::StreamEvent;
    use ww_can_bus::{
        Activity, BitTiming, BusError, BusLevel, BusLoad, CanConfig, CanError, CanErrorOwned,
        CanId, Capabilities, Direction, ErrorState, FdConfig, Filter, FilterAction, FilterMatch,
        FrameKind, MemoryLayout, Mode, NonMatching, Ohm, Options, Phase, ProtocolError,
        ProtocolStatus, RxOverflow, Status, Tdc, Termination, Timing, TimingLimits,
        TransceiverFault, TransceiverMode, TransceiverModes, TransceiverState, TxOrder, TxOutcome,
        TxPin,
    };
    use ww_si::{Baud, Hertz, Second};

    /// Controller clock of the mock: 80 MHz, as an STM32H7 FDCAN kernel clock.
    const CLOCK_HZ: u32 = 80_000_000;

    /// A frame as the mock keeps it.
    #[derive(Clone, Debug, PartialEq)]
    struct MockFrame {
        id: CanId,
        kind: FrameKind,
        data: Vec<u8>,
    }

    impl MockFrame {
        fn std(id: u16, data: &[u8]) -> Self {
            MockFrame {
                id: CanId::standard(id).unwrap(),
                kind: FrameKind::Data,
                data: data.to_vec(),
            }
        }
    }

    /// Something the mock's event loop sends before the next request.
    enum Pending {
        Rx(MockFrame, Option<u32>),
        TxEvent(Option<u32>, CanId, TxOutcome),
        BusError(BusError),
        Status,
        BusLoad(BusLoad),
        Transceiver,
    }

    /// Device behaviour and what it received, shared with the test.
    struct Model {
        config: CanConfig,
        status: Status,
        filters: Option<(Vec<Filter>, NonMatching, NonMatching, bool)>,
        rx_buffers: Vec<Option<MockFrame>>,
        /// `(index, frame, period)` of dedicated TX buffers written
        tx_buffers: Vec<(u16, Option<MockFrame>, Option<Second>)>,
        termination: Termination,
        /// Frames sent on `tx`, in order
        sent: Vec<MockFrame>,
        /// Frames sent but not on the bus yet (the mock holds them while `hold_tx` is set)
        queued: Vec<(MockFrame, Option<u32>)>,
        hold_tx: bool,
        pending: Vec<Pending>,
        layout: MemoryLayout,
        transceiver: TransceiverState,
        /// Nominal timing the other nodes of the mock's bus run at, for `detect_timing`
        bus_timing: BitTiming,
        /// Mode to return to on a wake-up
        before_sleep: Mode,
    }

    /// Message RAM of the mock in 32-bit words (as an STM32H7 FDCAN, 2560 shared by its instances).
    const RAM_WORDS: u32 = 2560;

    /// Words a layout takes: filters 1 / 2 words, RX / TX elements 2 header words + data, TX events 2 words.
    fn layout_words(l: &MemoryLayout) -> u32 {
        let element = 2 + l.element_data as u32 / 4;
        l.standard_filters as u32
            + 2 * l.extended_filters as u32
            + element
                * (l.rx_fifos[0] as u32
                    + l.rx_fifos[1] as u32
                    + l.rx_buffers as u32
                    + l.tx_buffers as u32
                    + l.tx_queue as u32)
            + 2 * l.tx_events as u32
    }

    fn default_layout() -> MemoryLayout {
        MemoryLayout {
            standard_filters: 3,
            extended_filters: 2,
            rx_fifos: [32, 32],
            rx_buffers: 4,
            tx_buffers: 4,
            tx_queue: 32,
            tx_events: 32,
            element_data: 64,
        }
    }

    fn default_config() -> CanConfig {
        CanConfig {
            mode: Mode::Off,
            nominal: Timing::Segments(BitTiming {
                prescaler: 1,
                seg1: 63,
                seg2: 16,
                sjw: 16,
            }),
            fd: None,
            options: Options {
                one_shot: false,
                presume_ack: false,
                triple_sample: false,
                tx_pause: false,
                tx_order: TxOrder::Fifo,
                rx_overflow: RxOverflow::KeepOldest,
                edge_filtering: false,
                protocol_exception: true,
                wake_on_bus: false,
            },
            bus_off_recovery: None,
        }
    }

    impl Default for Model {
        fn default() -> Self {
            Model {
                config: default_config(),
                status: Status {
                    state: ErrorState::Stopped,
                    tec: 0,
                    rec: 0,
                    last_error: None,
                    last_data_error: None,
                },
                filters: None,
                rx_buffers: vec![None; 4],
                tx_buffers: vec![],
                termination: Termination::Off,
                sent: vec![],
                queued: vec![],
                hold_tx: false,
                pending: vec![],
                layout: default_layout(),
                transceiver: TransceiverState {
                    mode: TransceiverMode::Normal,
                    fault: None,
                },
                bus_timing: segments_for(Baud::kilobaud(250u16), 875).unwrap(),
                before_sleep: Mode::Off,
            }
        }
    }

    impl Model {
        /// A frame on the bus: through the filters into `rx` or a dedicated buffer.
        fn receive(&mut self, frame: MockFrame) {
            let Some((filters, std, ext, reject_remote)) = &self.filters else {
                self.pending.push(Pending::Rx(frame, None));
                return;
            };
            if *reject_remote && matches!(frame.kind, FrameKind::Remote { .. }) {
                return;
            }
            let raw = frame.id.raw();
            let same_kind = |id: &CanId| id.is_extended() == frame.id.is_extended();
            let hit = filters.iter().position(|f| match f.matching {
                FilterMatch::Exact(id) => id == frame.id,
                FilterMatch::Mask { id, mask } => {
                    same_kind(&id) && (raw & mask.value()) == (id.raw() & mask.value())
                }
                FilterMatch::Range { from, to } => {
                    same_kind(&from) && (from.raw()..=to.raw()).contains(&raw)
                }
                FilterMatch::Unknown(_) => false,
            });
            match hit {
                Some(i) => match filters[i].action {
                    FilterAction::Fifo { .. } => {
                        self.pending.push(Pending::Rx(frame, Some(i as u32)))
                    }
                    FilterAction::Buffer(b) => self.rx_buffers[b as usize] = Some(frame),
                    _ => {}
                },
                None => {
                    let non_matching = if frame.id.is_extended() { ext } else { std };
                    if let NonMatching::Accept { .. } = non_matching {
                        self.pending.push(Pending::Rx(frame, None));
                    }
                }
            }
        }

        /// A frame from `tx` or a TX buffer goes on the bus (internal loopback).
        fn send(&mut self, frame: MockFrame, marker: Option<u32>) {
            let ok = match self.config.mode {
                Mode::Normal | Mode::InternalLoopback | Mode::ExternalLoopback => {
                    !matches!(frame.kind, FrameKind::Fd { .. }) || self.config.fd.is_some()
                }
                _ => false,
            };
            if !ok {
                self.pending
                    .push(Pending::TxEvent(marker, frame.id, TxOutcome::Rejected));
                return;
            }
            if self.hold_tx {
                self.queued.push((frame, marker));
                return;
            }
            self.sent.push(frame.clone());
            self.pending
                .push(Pending::TxEvent(marker, frame.id, TxOutcome::Sent));
            if self.config.mode == Mode::InternalLoopback {
                self.receive(frame);
            }
        }

        fn set_state(&mut self, state: ErrorState, tec: u8) {
            self.status.state = state;
            self.status.tec = tec;
            self.pending.push(Pending::Status);
        }
    }

    fn ohms_120() -> Ohm {
        Ohm::new(ww_si::Prefix::Unit, 120u8)
    }

    fn timing_limits() -> TimingLimits {
        TimingLimits {
            prescaler_min: 1,
            prescaler_max: 512,
            seg1_min: 2,
            seg1_max: 256,
            seg2_min: 1,
            seg2_max: 128,
            sjw_max: 128,
        }
    }

    /// M_CAN-like limits; [Capabilities::data] `None` on the minimal node (classic only).
    fn capabilities(full: bool) -> Capabilities<'static> {
        let termination: &'static [Ohm] = Box::leak(Box::new([ohms_120()]));
        Capabilities {
            clock: Hertz::megahertz(80u8),
            nominal: timing_limits(),
            data: full.then(|| TimingLimits {
                prescaler_max: 32,
                seg1_max: 32,
                seg2_max: 16,
                sjw_max: 16,
                ..timing_limits()
            }),
            tdc: full,
            restricted: full,
            listen_only: true,
            internal_loopback: true,
            external_loopback: full,
            test_mode: full,
            sleep: full,
            one_shot: true,
            presume_ack: false,
            triple_sample: false,
            tx_pause: full,
            tx_order: full,
            rx_overflow: full,
            edge_filtering: full,
            protocol_exception: full,
            wake_on_bus: full,
            long_dlc: false,
            auto_recovery: true,
            timestamps: Some(ww_can_bus::TimestampInfo {
                resolution: Second::microseconds(1u8),
                point: ww_can_bus::TimestampPoint::StartOfFrame,
            }),
            rx_fifos: if full { 2 } else { 1 },
            rx_fifo_depth: 64,
            tx_queue_depth: 32,
            tx_buffers: if full { 4 } else { 0 },
            rx_buffers: if full { 4 } else { 0 },
            standard_filters: if full { 3 } else { 0 },
            extended_filters: if full { 2 } else { 0 },
            filter_mask: full,
            filter_range: full,
            memory: full.then_some(RAM_WORDS),
            transceiver_modes: TransceiverModes {
                standby: full,
                silent: false,
                sleep: false,
            },
            termination: MaxLen::new(RefVec::Slice {
                slice: if full { termination } else { &[] },
            })
            .unwrap(),
            // the full node's figure is its group's
            performance: (!full).then_some(ww_can_bus::Performance {
                min_frames_per_second: Some(8000),
                min_bytes_per_second: None,
                independent_of_system: false,
            }),
        }
    }

    /// Picks segments for a bit rate and sample point: a prescaler of 1 or 2, segments within 0.5 % of the sample
    /// point. `None` when no such setting gives the rate exactly.
    fn segments_for(rate: Baud, sample_point: u16) -> Option<BitTiming> {
        let rate = rate.to_i128_in(ww_si::Prefix::Unit)? as u32;
        for prescaler in 1..=2u16 {
            let quanta = CLOCK_HZ / prescaler as u32 / rate;
            if quanta * prescaler as u32 * rate != CLOCK_HZ || !(8..=385).contains(&quanta) {
                continue;
            }
            let sample = (quanta * sample_point as u32 + 500) / 1000;
            let bt = BitTiming {
                prescaler,
                seg1: (sample - 1) as u16,
                seg2: (quanta - sample) as u8,
                sjw: (quanta - sample).min(128) as u8,
            };
            return Some(bt);
        }
        None
    }

    /// `configure` of both mocks: rates to segments, then the checks against `caps`.
    fn configure(
        caps: &Capabilities<'_>,
        config: CanConfig,
    ) -> Result<CanConfig, CanError<'static>> {
        let unsupported = || Err(CommonError::Unsupported.into());
        let resolve = |t: Timing| match t {
            Timing::Rate { rate, sample_point } => segments_for(rate, sample_point)
                .map(Timing::Segments)
                .ok_or(CanError::Common(CommonError::InvalidArgument {
                    arg: 0,
                    reason: MaxLen::new_unchecked("bit rate not reachable"),
                })),
            t => Ok(t),
        };
        let ok = match config.mode {
            Mode::Off | Mode::Config | Mode::Normal | Mode::ListenOnly | Mode::InternalLoopback => {
                true
            }
            Mode::Sleep => caps.sleep,
            Mode::Restricted => caps.restricted,
            Mode::ExternalLoopback => caps.external_loopback,
            Mode::Test => caps.test_mode,
            Mode::Unknown(_) => false,
        };
        if !ok || config.options.presume_ack || config.options.triple_sample {
            return unsupported();
        }
        if config.fd.is_some() && caps.data.is_none() {
            return unsupported();
        }
        let nominal = resolve(config.nominal)?;
        let fd = match config.fd {
            Some(fd) => Some(FdConfig {
                data: resolve(fd.data)?,
                // the mock measures 5 mtq of transceiver delay, and sets the offset at the sample point
                tdc: match fd.tdc {
                    Tdc::Auto => {
                        let data = resolve(fd.data)?.segments_or_default();
                        Tdc::Manual {
                            offset: (data.prescaler as u32 * (1 + data.seg1 as u32)) as u8,
                            filter: None,
                        }
                    }
                    tdc => tdc,
                },
                iso: fd.iso,
            }),
            None => None,
        };
        Ok(CanConfig {
            nominal,
            fd,
            ..config
        })
    }

    trait SegmentsOrDefault {
        fn segments_or_default(&self) -> BitTiming;
    }

    impl SegmentsOrDefault for Timing {
        fn segments_or_default(&self) -> BitTiming {
            self.segments().unwrap_or(BitTiming {
                prescaler: 1,
                seg1: 1,
                seg2: 1,
                sjw: 1,
            })
        }
    }

    fn state_after(mode: Mode) -> ErrorState {
        match mode {
            Mode::Off | Mode::Config => ErrorState::Stopped,
            Mode::Sleep => ErrorState::Sleeping,
            _ => ErrorState::Active,
        }
    }

    mod full_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use ww_client_server::credit::SinkCredit;
        use ww_stream::tx::Counted;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub can_rx_stream: Counted,
            pub can_tx_credit: SinkCredit,
            pub can_tx_events_stream: Counted,
            pub can_bus_errors_stream: Counted,
            pub stream_sessions: u32,
            pub start: std::time::Instant,
        }

        impl Server {
            /// The event loop's producer: frames received, TX events, bus errors and status changes.
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let (pending, status) = {
                    let mut m = self.m.lock().unwrap();
                    (std::mem::take(&mut m.pending), m.status)
                };
                let t = Some(Second::microseconds(self.start.elapsed().as_micros() as u32));
                let ser = api_impl::stream_data_ser();
                for p in pending {
                    let _ = match p {
                        Pending::Rx(f, filter) => {
                            let frame = ww_can_bus::RxFrame {
                                frame: ww_can_bus::Frame {
                                    id: f.id,
                                    kind: f.kind,
                                    data: MaxLen::new(RefVec::new_bytes(&f.data)).unwrap(),
                                },
                                filter: filter.map(UNib32),
                            };
                            ser.can()
                                .rx_send_blocking(&mut self.can_rx_stream, &frame, 1, t, out)
                        }
                        Pending::TxEvent(marker, id, outcome) => {
                            let ev = ww_can_bus::TxEvent {
                                marker: marker.map(UNib32),
                                id,
                                outcome,
                                // the mock's frames wait 120 µs in its queue
                                queued: (outcome == TxOutcome::Sent)
                                    .then(|| Second::microseconds(120u8)),
                            };
                            ser.can().tx_events_send_blocking(
                                &mut self.can_tx_events_stream,
                                &ev,
                                1,
                                t,
                                out,
                            )
                        }
                        Pending::BusError(e) => ser.can().bus_errors_send_blocking(
                            &mut self.can_bus_errors_stream,
                            &e,
                            1,
                            t,
                            out,
                        ),
                        Pending::Status => {
                            let _ = ser.can().status_send_blocking(&status, out);
                            Ok(())
                        }
                        Pending::BusLoad(load) => {
                            let _ = ser.can().bus_load_send_blocking(&load, out);
                            Ok(())
                        }
                        Pending::Transceiver => {
                            let state = self.m.lock().unwrap().transceiver;
                            let _ = ser.can().transceiver_state_send_blocking(&state, out);
                            Ok(())
                        }
                    };
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn can_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Capabilities<'_>> {
                Ready(capabilities(true))
            }

            fn can_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: CanConfig,
            ) -> RpcResult<Result<CanConfig, CanError<'_>>> {
                let r = configure(&capabilities(true), config);
                if let Ok(actual) = &r {
                    let mut m = self.m.lock().unwrap();
                    m.config = actual.clone();
                    m.set_state(state_after(actual.mode), 0);
                }
                Ready(r)
            }

            fn can_config(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<CanConfig> {
                Ready(self.m.lock().unwrap().config.clone())
            }

            fn get_can_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Status, ()> {
                Value(self.m.lock().unwrap().status)
            }

            fn can_rx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_can_rx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn can_tx_events(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_can_tx_events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn can_bus_errors(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_can_bus_errors(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn stream_can_rx(&mut self) -> &mut Counted {
                &mut self.can_rx_stream
            }

            fn credit_can_tx(&mut self) -> &mut SinkCredit {
                &mut self.can_tx_credit
            }

            fn sideband_can_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_can_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: ww_can_bus::TxFrame<'_>,
            ) {
                let frame = MockFrame {
                    id: value.frame.id,
                    kind: value.frame.kind,
                    data: value.frame.data.as_slice().to_vec(),
                };
                self.m
                    .lock()
                    .unwrap()
                    .send(frame, value.marker.map(|m| m.0));
                let len = self.can_tx_credit.last_item_len();
                self.can_tx_credit.free(len);
            }

            fn can_set_mode(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                mode: Mode,
            ) -> RpcResult<Result<Mode, CanError<'_>>> {
                let caps = capabilities(true);
                let mut config = self.m.lock().unwrap().config.clone();
                config.mode = mode;
                // the same checks as `configure`, the timing stays
                if let Err(e) = configure(&caps, config) {
                    return Ready(Err(e));
                }
                let mut m = self.m.lock().unwrap();
                if mode == Mode::Sleep {
                    m.before_sleep = m.config.mode;
                }
                m.config.mode = mode;
                let tec = m.status.tec;
                m.set_state(state_after(mode), tec);
                Ready(Ok(mode))
            }

            fn can_protocol_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<ProtocolStatus> {
                let mut m = self.m.lock().unwrap();
                let status = m.status;
                // last error codes reset on a read, as M_CAN's PSR
                m.status.last_error = None;
                m.status.last_data_error = None;
                Ready(ProtocolStatus {
                    status,
                    activity: if status.state == ErrorState::Stopped {
                        Activity::Synchronizing
                    } else {
                        Activity::Idle
                    },
                    error_log: Some(status.last_error.is_some() as u8),
                    last_fd_frame: None,
                    protocol_exception: false,
                    tdc_value: m.config.fd.map(|_| 5),
                    memory_errors: Some(0),
                })
            }

            fn get_can_bus_load(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<BusLoad, ()> {
                let frames = self.m.lock().unwrap().sent.len() as u32;
                Value(BusLoad {
                    window: Second::milliseconds(100u8),
                    load: 0,
                    frames,
                    errors: 0,
                })
            }

            fn can_detect_timing(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                candidates: MaxLen<RefVec<'_, Timing>, 16>,
                _per_candidate: Second,
            ) -> RpcResult<Result<Option<CanConfig>, CanError<'_>>> {
                if candidates.is_empty() {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked("no candidates"),
                    }
                    .into()));
                }
                let mut m = self.m.lock().unwrap();
                let bus = m.bus_timing;
                for t in candidates.iter() {
                    let t = t.unwrap();
                    let segments = match t {
                        Timing::Rate { rate, sample_point } => segments_for(rate, sample_point),
                        t => t.segments(),
                    };
                    // frames come in without errors only at the bus's bit rate and close to its sample point
                    if let Some(bt) = segments
                        && bt.bit_rate(CLOCK_HZ) == bus.bit_rate(CLOCK_HZ)
                        && bt.sample_point().abs_diff(bus.sample_point()) <= 50
                    {
                        m.config = CanConfig {
                            mode: Mode::ListenOnly,
                            nominal: Timing::Segments(bt),
                            fd: None,
                            ..m.config.clone()
                        };
                        m.set_state(ErrorState::Active, 0);
                        return Ready(Ok(Some(m.config.clone())));
                    }
                }
                Ready(Ok(None))
            }

            fn can_set_layout(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                layout: MemoryLayout,
            ) -> RpcResult<Result<MemoryLayout, CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if !m.queued.is_empty() {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked("frames queued"),
                    }
                    .into()));
                }
                if ![8, 12, 16, 20, 24, 32, 48, 64].contains(&layout.element_data) {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked("element_data"),
                    }
                    .into()));
                }
                let words = layout_words(&layout);
                if words > RAM_WORDS {
                    return Ready(Err(CanError::OutOfMemory {
                        available: RAM_WORDS,
                    }));
                }
                // off the bus for it and back, unless already in configuration mode
                m.layout = layout;
                Ready(Ok(layout))
            }

            fn can_set_transceiver(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                mode: TransceiverMode,
            ) -> RpcResult<Result<TransceiverMode, CanError<'_>>> {
                if !matches!(mode, TransceiverMode::Normal | TransceiverMode::Standby) {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                let mut m = self.m.lock().unwrap();
                m.transceiver.mode = mode;
                m.pending.push(Pending::Transceiver);
                Ready(Ok(mode))
            }

            fn get_can_transceiver_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<TransceiverState, ()> {
                Value(self.m.lock().unwrap().transceiver)
            }

            fn can_recover(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if m.status.state != ErrorState::BusOff {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked("not bus-off"),
                    }
                    .into()));
                }
                m.set_state(ErrorState::Active, 0);
                Ready(Ok(()))
            }

            fn can_tx_abort(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<u32> {
                let mut m = self.m.lock().unwrap();
                let queued = std::mem::take(&mut m.queued);
                for (f, marker) in &queued {
                    m.pending
                        .push(Pending::TxEvent(*marker, f.id, TxOutcome::Aborted));
                }
                Ready(queued.len() as u32)
            }

            fn stream_can_tx_events(&mut self) -> &mut Counted {
                &mut self.can_tx_events_stream
            }

            fn stream_can_bus_errors(&mut self) -> &mut Counted {
                &mut self.can_bus_errors_stream
            }

            fn can_set_filters(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                filters: ww_can_bus::FilterConfig<'_>,
            ) -> RpcResult<Result<(), CanError<'_>>> {
                let caps = capabilities(true);
                let layout = self.m.lock().unwrap().layout;
                let list: Vec<Filter> = filters.filters.iter().map(|f| f.unwrap()).collect();
                let ext = |f: &&Filter| match f.matching {
                    FilterMatch::Exact(id)
                    | FilterMatch::Mask { id, .. }
                    | FilterMatch::Range { from: id, .. } => id.is_extended(),
                    FilterMatch::Unknown(_) => false,
                };
                let n_ext = list.iter().filter(ext).count() as u16;
                let n_std = list.len() as u16 - n_ext;
                // as many as the memory layout holds
                if n_std > layout.standard_filters {
                    return Ready(Err(CanError::OutOfFilters {
                        max: layout.standard_filters,
                    }));
                }
                if n_ext > layout.extended_filters {
                    return Ready(Err(CanError::OutOfFilters {
                        max: layout.extended_filters,
                    }));
                }
                for f in &list {
                    let bad = match f.action {
                        FilterAction::Fifo { fifo, .. } => fifo >= caps.rx_fifos,
                        FilterAction::Buffer(n) => n >= caps.rx_buffers,
                        _ => false,
                    };
                    if bad {
                        return Ready(Err(CommonError::InvalidArgument {
                            arg: 0,
                            reason: MaxLen::new_unchecked("no such FIFO or buffer"),
                        }
                        .into()));
                    }
                }
                self.m.lock().unwrap().filters = Some((
                    list,
                    filters.non_matching_standard,
                    filters.non_matching_extended,
                    filters.reject_remote,
                ));
                Ready(Ok(()))
            }

            fn can_rx_buffer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: u16,
            ) -> RpcResult<Result<Option<ww_can_bus::BufferedFrame<'_>>, CanError<'_>>>
            {
                let mut m = self.m.lock().unwrap();
                let Some(slot) = m.rx_buffers.get_mut(index as usize) else {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                };
                // the frame has to outlive the lock: leak it, a test runs only a few of these
                let frame = slot.take().map(|f| {
                    let data: &'static [u8] = Box::leak(f.data.into_boxed_slice());
                    ww_can_bus::BufferedFrame {
                        frame: ww_can_bus::Frame {
                            id: f.id,
                            kind: f.kind,
                            data: MaxLen::new(RefVec::new_bytes(data)).unwrap(),
                        },
                        time: Some(Second::microseconds(self.start.elapsed().as_micros() as u32)),
                    }
                });
                Ready(Ok(frame))
            }

            fn can_tx_buffer(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: u16,
                frame: Option<ww_can_bus::TxFrame<'_>>,
                period: Option<Second>,
            ) -> RpcResult<Result<(), CanError<'_>>> {
                let mut m = self.m.lock().unwrap();
                if index >= capabilities(true).tx_buffers {
                    return Ready(Err(CommonError::InvalidArgument {
                        arg: 0,
                        reason: MaxLen::new_unchecked("no such buffer"),
                    }
                    .into()));
                }
                if m.status.state == ErrorState::BusOff {
                    return Ready(Err(CanError::BusOff));
                }
                if matches!(m.config.mode, Mode::Off | Mode::ListenOnly) {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked(""),
                    }
                    .into()));
                }
                let f = frame.as_ref().map(|f| MockFrame {
                    id: f.frame.id,
                    kind: f.frame.kind,
                    data: f.frame.data.as_slice().to_vec(),
                });
                m.tx_buffers.push((index, f.clone(), period));
                // sent once now; a cyclic one too, then again every period (not simulated)
                if let (Some(f), Some(tx)) = (f, frame) {
                    m.send(f, tx.marker.map(|m| m.0));
                }
                Ready(Ok(()))
            }

            fn can_set_termination(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                termination: Termination,
            ) -> RpcResult<Result<Termination, CanError<'_>>> {
                if let Termination::On(r) = termination
                    && r != ohms_120()
                {
                    return Ready(Err(CommonError::Unsupported.into()));
                }
                self.m.lock().unwrap().termination = termination;
                Ready(Ok(termination))
            }

            fn get_can_termination(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Termination, ()> {
                Value(self.m.lock().unwrap().termination)
            }

            fn can_test_pins(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                tx: TxPin,
            ) -> RpcResult<Result<BusLevel, CanError<'_>>> {
                if self.m.lock().unwrap().config.mode != Mode::Test {
                    return Ready(Err(CommonError::WrongState {
                        reason: MaxLen::new_unchecked("not in Mode::Test"),
                    }
                    .into()));
                }
                // alone on the bus: RX follows TX
                Ready(Ok(match tx {
                    TxPin::Dominant => BusLevel::Dominant,
                    _ => BusLevel::Recessive,
                }))
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_can_api :: IoCanTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                // debug_to_file = "../../target/tests_io_can_server.rs"
            );
        }

        impl TestProcessEvents for Server {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                // the mock's producer runs between requests
                self.produce(out);
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    /// A simple node: only the six required items, every optional one left out with `omit`.
    mod minimal_server {
        use super::*;
        use tests_common::TestProcessEvents;
        use ww_client_server::credit::SinkCredit;
        use ww_stream::tx::Counted;

        pub struct Server {
            pub m: Arc<Mutex<Model>>,
            pub can_rx_stream: Counted,
            pub can_tx_credit: SinkCredit,
            pub stream_sessions: u32,
        }

        impl Server {
            fn produce(&mut self, out: &mut impl BlockingEventOut) {
                let pending = std::mem::take(&mut self.m.lock().unwrap().pending);
                let ser = api_impl::stream_data_ser();
                for p in pending {
                    if let Pending::Rx(f, _) = p {
                        let frame = ww_can_bus::RxFrame {
                            frame: ww_can_bus::Frame {
                                id: f.id,
                                kind: f.kind,
                                data: MaxLen::new(RefVec::new_bytes(&f.data)).unwrap(),
                            },
                            filter: None,
                        };
                        // no timestamps on this one
                        let _ = ser.can().rx_send_blocking(
                            &mut self.can_rx_stream,
                            &frame,
                            1,
                            None,
                            out,
                        );
                    }
                }
            }

            fn new_stream_session(&mut self) -> u32 {
                self.stream_sessions = self.stream_sessions.wrapping_add(1);
                self.stream_sessions
            }

            fn can_capabilities(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Capabilities<'_>> {
                Ready(capabilities(false))
            }

            fn can_configure(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                config: CanConfig,
            ) -> RpcResult<Result<CanConfig, CanError<'_>>> {
                let r = configure(&capabilities(false), config);
                if let Ok(actual) = &r {
                    let mut m = self.m.lock().unwrap();
                    m.config = actual.clone();
                    m.status.state = state_after(actual.mode);
                }
                Ready(r)
            }

            fn can_config(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<CanConfig> {
                Ready(self.m.lock().unwrap().config.clone())
            }

            fn get_can_status(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> GetResult<Status, ()> {
                Value(self.m.lock().unwrap().status)
            }

            fn can_rx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
                Ready(())
            }

            fn close_can_rx(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn stream_can_rx(&mut self) -> &mut Counted {
                &mut self.can_rx_stream
            }

            fn credit_can_tx(&mut self) -> &mut SinkCredit {
                &mut self.can_tx_credit
            }

            fn sideband_can_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_can_tx(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: ww_can_bus::TxFrame<'_>,
            ) {
                let frame = MockFrame {
                    id: value.frame.id,
                    kind: value.frame.kind,
                    data: value.frame.data.as_slice().to_vec(),
                };
                self.m.lock().unwrap().send(frame, None);
                let len = self.can_tx_credit.last_item_len();
                self.can_tx_credit.free(len);
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                io_can_api :: IoCanTest for super::Server,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                omit = [
                    "can.set_mode", "can.recover", "can.tx_abort", "can.tx_events", "can.bus_errors",
                    "can.protocol_status", "can.bus_load", "can.detect_timing", "can.filters",
                    "can.set_layout", "can.rx_buffers", "can.tx_buffers", "can.transceiver",
                    "can.terminator", "can.test_pins"
                ],
            );
        }

        impl TestProcessEvents for Server {
            type Medium = ();

            fn process_request_bytes<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
                medium: (),
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.produce(out);
                self.process_request_bytes(bytes, scratch, out, medium)
            }
        }
    }

    mod client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct StdClient {
            pub cmd: Commander,
        }

        impl WwClient for StdClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                io_can_api :: IoCanTest for super::StdClient,
                client = "std_client",
                // debug_to_file = "../../target/tests_io_can_client.rs"
            );
        }
    }

    use client::StdClient;

    /// The mock's TX queue: 32 frames of up to 64 bytes.
    const TX_BUFFER: u32 = 32 * 72;

    fn start_full(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(Model::default()));
        let server = full_server::Server {
            m: m.clone(),
            can_rx_stream: ww_stream::tx::Counted::new(),
            can_tx_credit: ww_client_server::credit::SinkCredit::new(TX_BUFFER),
            can_tx_events_stream: ww_stream::tx::Counted::new(),
            can_bus_errors_stream: ww_stream::tx::Counted::new(),
            stream_sessions: 0,
            start: std::time::Instant::now(),
        };
        let device = tests_common::start_device(
            path,
            server,
            io_can_api::IO_CAN_TEST_FULL_GID,
            full_server::api_impl::api_hash(),
        );
        (device, m)
    }

    fn start_minimal(path: &str) -> (TestDevice, Arc<Mutex<Model>>) {
        let _ = tracing_subscriber::fmt::try_init();
        let m = Arc::new(Mutex::new(Model::default()));
        let server = minimal_server::Server {
            m: m.clone(),
            can_rx_stream: ww_stream::tx::Counted::new(),
            can_tx_credit: ww_client_server::credit::SinkCredit::new(TX_BUFFER),
            stream_sessions: 0,
        };
        let device = tests_common::start_device(
            path,
            server,
            io_can_api::IO_CAN_TEST_FULL_GID,
            minimal_server::api_impl::api_hash(),
        );
        (device, m)
    }

    async fn connect(device: &TestDevice) -> StdClient {
        StdClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect")
    }

    /// Wait for the device thread to process what was sent to it
    async fn eventually(m: &Arc<Mutex<Model>>, f: impl Fn(&Model) -> bool) {
        for _ in 0..500 {
            if f(&m.lock().unwrap()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("condition not met within 500ms");
    }

    fn tx_frame(frame: &MockFrame, marker: Option<u32>) -> ww_can_bus::TxFrameOwned {
        ww_can_bus::TxFrameOwned {
            frame: ww_can_bus::FrameOwned {
                id: frame.id,
                kind: frame.kind,
                data: MaxLen::new(frame.data.clone()).unwrap(),
            },
            marker: marker.map(UNib32),
        }
    }

    fn loopback(nominal: Timing, fd: Option<FdConfig>) -> CanConfig {
        CanConfig {
            mode: Mode::InternalLoopback,
            nominal,
            fd,
            ..default_config()
        }
    }

    const RATE_500K: fn() -> Timing = || Timing::Rate {
        rate: Baud::kilobaud(500u16),
        sample_point: 875,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capabilities_and_configure() {
        let (device, m) = start_full("io_can/configure");
        let client = connect(&device).await;

        let caps = client.can().capabilities().call().await.unwrap();
        assert_eq!(caps.clock, Hertz::megahertz(80u8));
        assert!(caps.data.is_some());
        assert_eq!(caps.termination.len(), 1);

        // a bit rate in, segments out: 80 MHz / 500 kbit/s = 160 quanta, 140 before the sample point
        let fd = FdConfig {
            data: Timing::Rate {
                rate: Baud::megabaud(2u8),
                sample_point: 750,
            },
            tdc: Tdc::Auto,
            iso: true,
        };
        let actual = client
            .can()
            .configure(loopback(RATE_500K(), Some(fd)))
            .call()
            .await
            .unwrap();
        let nominal = actual.nominal.segments().expect("answered as segments");
        assert_eq!(
            (nominal.prescaler, nominal.seg1, nominal.seg2),
            (1, 139, 20)
        );
        assert_eq!(nominal.bit_rate(CLOCK_HZ), Some(500_000));
        assert_eq!(nominal.sample_point(), 875);
        let data = actual.fd.unwrap().data.segments().unwrap();
        assert_eq!(data.bit_rate(CLOCK_HZ), Some(2_000_000));
        // TDC auto: the offset at the data sample point, in clock periods
        assert_eq!(
            actual.fd.unwrap().tdc,
            Tdc::Manual {
                offset: 30,
                filter: None
            }
        );
        assert_eq!(m.lock().unwrap().config.mode, Mode::InternalLoopback);
        // a host attaching later reads the same
        assert_eq!(client.can().config().call().await.unwrap(), actual);

        // 333 kbit/s isn't reachable from 80 MHz exactly
        let r = client
            .can()
            .configure(loopback(
                Timing::Rate {
                    rate: Baud::baud(333_333u32),
                    sample_point: 875,
                },
                None,
            ))
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));

        // an option this controller doesn't have
        let mut config = loopback(RATE_500K(), None);
        config.options.presume_ack = true;
        let r = client.can().configure(config).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn loopback_frames_and_tx_events() {
        let (device, m) = start_full("io_can/loopback");
        let client = connect(&device).await;
        let fd = FdConfig {
            data: Timing::Rate {
                rate: Baud::megabaud(4u8),
                sample_point: 800,
            },
            tdc: Tdc::Off,
            iso: true,
        };
        client
            .can()
            .configure(loopback(RATE_500K(), Some(fd)))
            .call()
            .await
            .unwrap();

        let mut rx = client.can().rx().call().await.unwrap();
        let mut events = client.can().tx_events().call().await.unwrap();
        let mut tx = client.can().tx().await.unwrap();

        let frames = [
            MockFrame::std(0x123, &[1, 2, 3]),
            MockFrame {
                id: CanId::extended(0x1ABC_DEF0).unwrap(),
                kind: FrameKind::Fd {
                    brs: true,
                    esi: false,
                },
                data: (0..64).collect(),
            },
            MockFrame {
                id: CanId::standard(0x7FF).unwrap(),
                kind: FrameKind::Remote {
                    len: U4::new(8).unwrap(),
                },
                data: vec![],
            },
        ];
        for (i, f) in frames.iter().enumerate() {
            tx.send_owned(tx_frame(f, Some(100 + i as u32)))
                .await
                .unwrap();
        }
        tx.flush().await.unwrap();
        eventually(&m, |m| m.sent.len() == 3).await;
        // any request lets the mock's event loop send what came back
        client.can().config().call().await.unwrap();

        for (i, f) in frames.iter().enumerate() {
            match rx.recv_event().await.unwrap() {
                StreamEvent::Item { pos, t, value, .. } => {
                    assert_eq!(pos, i as u64);
                    assert!(t.is_some(), "timestamped");
                    assert_eq!(value.frame.id, f.id);
                    assert_eq!(value.frame.kind, f.kind);
                    assert_eq!(value.frame.data.as_slice(), f.data.as_slice());
                    assert_eq!(value.filter, None);
                }
                ev => panic!("{ev:?}"),
            }
            let ev = events.recv().await.unwrap();
            assert_eq!(ev.marker, Some(UNib32(100 + i as u32)));
            assert_eq!(ev.id, f.id);
            assert_eq!(ev.outcome, TxOutcome::Sent);
            // measured on the device
            assert_eq!(ev.queued, Some(Second::microseconds(120u8)));
        }

        // without FD, an FD frame is rejected
        client
            .can()
            .configure(loopback(RATE_500K(), None))
            .call()
            .await
            .unwrap();
        tx.send_owned(tx_frame(&frames[1], Some(7))).await.unwrap();
        tx.flush().await.unwrap();
        client.can().config().call().await.unwrap();
        let ev = events.recv().await.unwrap();
        assert_eq!(
            (ev.marker, ev.outcome),
            (Some(UNib32(7)), TxOutcome::Rejected)
        );

        // frames held in the queue are aborted
        m.lock().unwrap().hold_tx = true;
        tx.send_owned(tx_frame(&frames[0], Some(8))).await.unwrap();
        tx.send_owned(tx_frame(&frames[0], Some(9))).await.unwrap();
        tx.flush().await.unwrap();
        eventually(&m, |m| m.queued.len() == 2).await;
        assert_eq!(client.can().tx_abort().call().await.unwrap(), 2);
        client.can().config().call().await.unwrap();
        for marker in [8, 9] {
            let ev = events.recv().await.unwrap();
            assert_eq!(
                (ev.marker, ev.outcome),
                (Some(UNib32(marker)), TxOutcome::Aborted)
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn filters_route_frames() {
        let (device, m) = start_full("io_can/filters");
        let client = connect(&device).await;
        client
            .can()
            .configure(loopback(RATE_500K(), None))
            .call()
            .await
            .unwrap();

        let std = |id| CanId::standard(id).unwrap();
        let filter = |matching, action| Filter { matching, action };
        let config = |filters: Vec<Filter>| ww_can_bus::FilterConfigOwned {
            filters: MaxLen::new(filters).unwrap(),
            non_matching_standard: NonMatching::Reject,
            non_matching_extended: NonMatching::Accept { fifo: 1 },
            reject_remote: false,
        };
        let filters = vec![
            // 0x100 only into dedicated buffer 2
            filter(FilterMatch::Exact(std(0x100)), FilterAction::Buffer(2)),
            // 0x200..=0x20F to FIFO 0
            filter(
                FilterMatch::Range {
                    from: std(0x200),
                    to: std(0x20F),
                },
                FilterAction::Fifo {
                    fifo: 0,
                    high_priority: false,
                },
            ),
            // 0x3x0 rejected
            filter(
                FilterMatch::Mask {
                    id: std(0x300),
                    mask: U29::new(0x70F).unwrap(),
                },
                FilterAction::Reject,
            ),
        ];
        client
            .can()
            .set_filters(config(filters.clone()))
            .call()
            .await
            .unwrap();

        // one standard filter too many
        let mut too_many = filters.clone();
        too_many.push(filter(
            FilterMatch::Exact(std(1)),
            FilterAction::Fifo {
                fifo: 0,
                high_priority: false,
            },
        ));
        let r = client.can().set_filters(config(too_many)).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::OutOfFilters { max: 3 })
        ));
        // no FIFO 2 on this controller
        let r = client
            .can()
            .set_filters(config(vec![filter(
                FilterMatch::Exact(std(1)),
                FilterAction::Fifo {
                    fifo: 2,
                    high_priority: false,
                },
            )]))
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));

        let mut rx = client.can().rx().call().await.unwrap();
        let mut tx = client.can().tx().await.unwrap();
        let ext = MockFrame {
            id: CanId::extended(0x18FF_0001).unwrap(),
            kind: FrameKind::Data,
            data: vec![9],
        };
        for f in [
            MockFrame::std(0x100, &[1]),
            MockFrame::std(0x205, &[2]),
            MockFrame::std(0x310, &[3]),
            MockFrame::std(0x400, &[4]),
            ext.clone(),
        ] {
            tx.send_owned(tx_frame(&f, None)).await.unwrap();
        }
        tx.flush().await.unwrap();
        eventually(&m, |m| m.sent.len() == 5).await;
        client.can().config().call().await.unwrap();

        // 0x205 by filter 1, the extended frame as non-matching; the rest is in the buffer or dropped
        let a = rx.recv().await.unwrap();
        assert_eq!((a.frame.id, a.filter), (std(0x205), Some(UNib32(1))));
        let b = rx.recv().await.unwrap();
        assert_eq!((b.frame.id, b.filter), (ext.id, None));

        let buffered = client.can().rx_buffer(2).call().await.unwrap();
        let buffered = buffered.expect("0x100 went into buffer 2");
        assert_eq!(buffered.frame.id, std(0x100));
        assert!(buffered.time.is_some());
        // read once
        assert!(client.can().rx_buffer(2).call().await.unwrap().is_none());
        let r = client.can().rx_buffer(9).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn status_bus_off_and_recovery() {
        let (device, m) = start_full("io_can/status");
        let client = connect(&device).await;
        let mut status = client.can().observe_status().await.unwrap();
        let mut errors = client.can().bus_errors().call().await.unwrap();

        client
            .can()
            .configure(CanConfig {
                mode: Mode::Normal,
                nominal: RATE_500K(),
                ..default_config()
            })
            .call()
            .await
            .unwrap();
        assert_eq!(
            client.can().read_status().read().await.unwrap().state,
            ErrorState::Active
        );
        let r = client.can().recover().call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));

        // nobody acknowledges: ACK errors until bus-off
        {
            let mut m = m.lock().unwrap();
            m.pending.push(Pending::BusError(BusError {
                kind: ProtocolError::Ack,
                phase: Phase::Arbitration,
                dir: Direction::Tx,
            }));
            m.status.last_error = Some(ProtocolError::Ack);
            m.set_state(ErrorState::BusOff, 255);
        }
        // each status change goes to observers; the first is from `configure`
        client.can().config().call().await.unwrap();
        let e = errors.recv().await.unwrap();
        assert_eq!((e.kind, e.dir), (ProtocolError::Ack, Direction::Tx));
        let mut s = status.recv().await.unwrap();
        while s.state != ErrorState::BusOff {
            s = status.recv().await.unwrap();
        }
        assert_eq!((s.tec, s.last_error), (255, Some(ProtocolError::Ack)));

        // a dedicated TX buffer can't send while bus-off
        let r = client
            .can()
            .tx_buffer(0, Some(tx_frame(&MockFrame::std(1, &[]), None)), None)
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::BusOff)
        ));

        client.can().recover().call().await.unwrap();
        client.can().config().call().await.unwrap();
        assert_eq!(status.recv().await.unwrap().state, ErrorState::Active);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tx_buffers_termination_and_test_pins() {
        let (device, m) = start_full("io_can/extras");
        let client = connect(&device).await;
        client
            .can()
            .configure(loopback(RATE_500K(), None))
            .call()
            .await
            .unwrap();

        // a cyclic frame every 100 ms, then cleared
        let period = Second::milliseconds(100u8);
        let heartbeat = MockFrame::std(0x701, &[5]);
        client
            .can()
            .tx_buffer(1, Some(tx_frame(&heartbeat, Some(1))), Some(period))
            .call()
            .await
            .unwrap();
        client.can().tx_buffer(1, None, None).call().await.unwrap();
        {
            let m = m.lock().unwrap();
            assert_eq!(m.tx_buffers[0], (1, Some(heartbeat.clone()), Some(period)));
            assert_eq!(m.tx_buffers[1], (1, None, None));
            assert_eq!(m.sent, vec![heartbeat]);
        }
        let r = client.can().tx_buffer(4, None, None).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));

        let on = Termination::On(ohms_120());
        assert_eq!(client.can().set_termination(on).call().await.unwrap(), on);
        assert_eq!(client.can().read_termination().read().await.unwrap(), on);
        let r = client
            .can()
            .set_termination(Termination::On(Ohm::new(ww_si::Prefix::Unit, 60u8)))
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));

        // test pins only in Mode::Test
        let r = client.can().test_pins(TxPin::Dominant).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::WrongState { .. }))
        ));
        client
            .can()
            .configure(CanConfig {
                mode: Mode::Test,
                ..loopback(RATE_500K(), None)
            })
            .call()
            .await
            .unwrap();
        assert_eq!(
            client
                .can()
                .test_pins(TxPin::Dominant)
                .call()
                .await
                .unwrap(),
            BusLevel::Dominant
        );
        assert_eq!(
            client
                .can()
                .test_pins(TxPin::Recessive)
                .call()
                .await
                .unwrap(),
            BusLevel::Recessive
        );
    }

    /// A bus debugger keeps every step in hand: explicit modes, configuration-only steps in `Mode::Config`, the raw
    /// protocol state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn debugger_steps() {
        let (device, m) = start_full("io_can/debugger");
        let client = connect(&device).await;
        client
            .can()
            .configure(CanConfig {
                mode: Mode::Normal,
                nominal: RATE_500K(),
                ..default_config()
            })
            .call()
            .await
            .unwrap();

        // off the bus for configuration-only steps
        assert_eq!(
            client.can().set_mode(Mode::Config).call().await.unwrap(),
            Mode::Config
        );
        assert_eq!(
            client.can().read_status().read().await.unwrap().state,
            ErrorState::Stopped
        );
        let big = MemoryLayout {
            rx_fifos: [64, 64],
            tx_buffers: 32,
            ..default_layout()
        };
        let r = client.can().set_layout(big).call().await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::OutOfMemory {
                available: RAM_WORDS
            })
        ));
        let more_filters = MemoryLayout {
            standard_filters: 8,
            ..default_layout()
        };
        assert_eq!(
            client.can().set_layout(more_filters).call().await.unwrap(),
            more_filters
        );
        // four standard filters fit the new layout
        let filters = (0..4)
            .map(|i| Filter {
                matching: FilterMatch::Exact(CanId::standard(0x100 + i).unwrap()),
                action: FilterAction::Fifo {
                    fifo: 0,
                    high_priority: i == 0,
                },
            })
            .collect();
        client
            .can()
            .set_filters(ww_can_bus::FilterConfigOwned {
                filters: MaxLen::new(filters).unwrap(),
                non_matching_standard: NonMatching::Reject,
                non_matching_extended: NonMatching::Reject,
                reject_remote: true,
            })
            .call()
            .await
            .unwrap();
        // still in configuration mode, then on the bus in any mode, timing kept
        assert_eq!(
            client.can().config().call().await.unwrap().mode,
            Mode::Config
        );
        for mode in [Mode::Restricted, Mode::ListenOnly, Mode::Normal] {
            assert_eq!(client.can().set_mode(mode).call().await.unwrap(), mode);
            let config = client.can().config().call().await.unwrap();
            assert_eq!(config.mode, mode);
            assert_eq!(
                config.nominal.segments().unwrap().bit_rate(CLOCK_HZ),
                Some(500_000)
            );
        }

        // the raw protocol state; last error codes reset on a read
        m.lock().unwrap().status.last_error = Some(ProtocolError::Bit0);
        let ps = client.can().protocol_status().call().await.unwrap();
        assert_eq!(ps.status.last_error, Some(ProtocolError::Bit0));
        assert_eq!(ps.error_log, Some(1));
        assert_eq!(ps.activity, Activity::Idle);
        assert_eq!(ps.tdc_value, None, "no FD, no TDC");
        let ps = client.can().protocol_status().call().await.unwrap();
        assert_eq!(ps.status.last_error, None);
    }

    /// Measured on the device: the bit timing of an unknown bus, bus load.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn detect_timing_and_bus_load() {
        let (device, m) = start_full("io_can/detect");
        let client = connect(&device).await;

        let rate = |kbit: u16, sample_point| Timing::Rate {
            rate: Baud::kilobaud(kbit),
            sample_point,
        };
        let candidates = |list: Vec<Timing>| MaxLen::new(list).unwrap();
        let per = Second::milliseconds(200u8);
        // the mock's bus runs at 250 kbit/s, 87.5 %
        let found = client
            .can()
            .detect_timing(
                candidates(vec![
                    rate(1000, 800),
                    rate(500, 875),
                    rate(250, 875),
                    rate(125, 875),
                ]),
                per,
            )
            .call()
            .await
            .unwrap()
            .expect("250 kbit/s found");
        assert_eq!(found.mode, Mode::ListenOnly);
        assert_eq!(
            found.nominal.segments().unwrap().bit_rate(CLOCK_HZ),
            Some(250_000)
        );
        assert_eq!(client.can().config().call().await.unwrap(), found);
        let none = client
            .can()
            .detect_timing(candidates(vec![rate(1000, 800)]), per)
            .call()
            .await
            .unwrap();
        assert_eq!(none, None);
        let r = client
            .can()
            .detect_timing(candidates(vec![]), per)
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(
                CommonErrorOwned::InvalidArgument { .. }
            ))
        ));

        let mut load = client.can().observe_bus_load().await.unwrap();
        let measured = BusLoad {
            window: Second::milliseconds(100u8),
            load: 347,
            frames: 120,
            errors: 1,
        };
        m.lock().unwrap().pending.push(Pending::BusLoad(measured));
        client.can().config().call().await.unwrap();
        assert_eq!(load.recv().await.unwrap(), measured);
        assert_eq!(
            client.can().read_bus_load().read().await.unwrap().window,
            Second::milliseconds(100u8)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sleep_wake_and_transceiver() {
        let (device, m) = start_full("io_can/sleep");
        let client = connect(&device).await;
        let mut status = client.can().observe_status().await.unwrap();
        let mut config = CanConfig {
            mode: Mode::Normal,
            nominal: RATE_500K(),
            ..default_config()
        };
        config.options.wake_on_bus = true;
        client.can().configure(config).call().await.unwrap();
        client.can().set_mode(Mode::Sleep).call().await.unwrap();
        client
            .can()
            .set_transceiver(TransceiverMode::Standby)
            .call()
            .await
            .unwrap();
        client.can().config().call().await.unwrap();
        let mut s = status.recv().await.unwrap();
        while s.state != ErrorState::Sleeping {
            s = status.recv().await.unwrap();
        }

        // activity on the bus wakes it up, back in the mode before sleeping
        {
            let mut m = m.lock().unwrap();
            m.config.mode = m.before_sleep;
            m.transceiver.mode = TransceiverMode::Normal;
            m.set_state(ErrorState::Active, 0);
        }
        client.can().config().call().await.unwrap();
        assert_eq!(status.recv().await.unwrap().state, ErrorState::Active);
        assert_eq!(
            client.can().config().call().await.unwrap().mode,
            Mode::Normal
        );

        let r = client
            .can()
            .set_transceiver(TransceiverMode::Silent)
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));
        // a wiring fault reaches observers
        let mut trx = client.can().observe_transceiver_state().await.unwrap();
        {
            let mut m = m.lock().unwrap();
            m.transceiver.fault = Some(TransceiverFault::CanHighShortToGround);
            m.pending.push(Pending::Transceiver);
        }
        client.can().config().call().await.unwrap();
        let mut t = trx.recv().await.unwrap();
        while t.fault.is_none() {
            t = trx.recv().await.unwrap();
        }
        assert_eq!(t.fault, Some(TransceiverFault::CanHighShortToGround));
        assert_eq!(
            client.can().read_transceiver_state().read().await.unwrap(),
            t
        );
    }

    /// The six required items are all a simple node needs; the optional ones fail locally, known from the device's
    /// introspection data.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn minimal_node() {
        let (device, m) = start_minimal("io_can/minimal");
        let client = connect(&device).await;

        let caps = client.can().capabilities().call().await.unwrap();
        assert!(caps.data.is_none(), "classic only");
        // no FD on this one
        let r = client
            .can()
            .configure(loopback(
                RATE_500K(),
                Some(FdConfig {
                    data: RATE_500K(),
                    tdc: Tdc::Off,
                    iso: true,
                }),
            ))
            .call()
            .await;
        assert!(matches!(
            r.unwrap_err().device(),
            Some(CanErrorOwned::Common(CommonErrorOwned::Unsupported))
        ));
        client
            .can()
            .configure(loopback(RATE_500K(), None))
            .call()
            .await
            .unwrap();
        assert_eq!(
            client.can().read_status().read().await.unwrap().state,
            ErrorState::Active
        );

        let mut rx = client.can().rx().call().await.unwrap();
        let mut tx = client.can().tx().await.unwrap();
        tx.send_owned(tx_frame(&MockFrame::std(0x42, &[0xAA, 0xBB]), None))
            .await
            .unwrap();
        tx.flush().await.unwrap();
        eventually(&m, |m| m.sent.len() == 1).await;
        client.can().config().call().await.unwrap();
        match rx.recv_event().await.unwrap() {
            StreamEvent::Item { t, value, .. } => {
                assert!(t.is_none(), "this node has no timestamps");
                assert_eq!(value.frame.id, CanId::standard(0x42).unwrap());
                assert_eq!(value.frame.data.as_slice(), &[0xAA, 0xBB]);
            }
            ev => panic!("{ev:?}"),
        }

        let not_here = |e: &wire_weaver_client::Error| {
            matches!(e, wire_weaver_client::Error::NotImplementedByDevice { .. })
        };
        // methods returning a `Result` answer `CallError::Unimplemented`
        let unimplemented = |e: wire_weaver_client::CallError<CanErrorOwned>| {
            matches!(e, wire_weaver_client::CallError::Unimplemented { .. })
        };
        assert!(unimplemented(
            client.can().recover().call().await.unwrap_err()
        ));
        assert!(not_here(&client.can().tx_abort().call().await.unwrap_err()));
        assert!(unimplemented(
            client.can().rx_buffer(0).call().await.unwrap_err()
        ));
        assert!(not_here(
            &client.can().read_termination().read().await.unwrap_err()
        ));
    }
}
