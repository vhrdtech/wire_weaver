//! Firmware of line 2, `thermo_api` 2.1.0 at w2.1, simulated.

use crate::Shared;
use thermo_api_w2::{Command, Config, Mode, RateError, Sample, SelfTestError};
use wire_weaver::prelude::*;
use wire_weaver::ww_version::WireVersion;
use ww_si::prefix::{Centi, Milli, One};
use ww_si::units::{Kelvin, Ratio, Second};
use ww_si::{Fixed, Range};

pub struct Server {
    pub observe: api_impl::ObserveState,
    pub shared: Shared,
    mode: Mode,
    gain: Fixed<Ratio, Centi, u16, Range<100, 1600>>,
    /// Added to every sample, 0.001 K
    offset: i32,
    seq: u16,
    enabled: [bool; 4],
    indication: ww_indication::Mode,
    buf: Vec<u8>,
}

impl Server {
    fn sample(&mut self, channel: u8) -> Sample {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        Sample {
            seq: ww_si::Seq::new(seq),
            input: ww_si::IndexFrom1::try_new(channel + 1).unwrap(),
            // 21.500 °C in 0.001 K
            temp: ww_si::Point::from_raw(21500 + self.offset),
        }
    }

    async fn configure(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        config: Config,
    ) -> RpcResult<Result<(), RateError>> {
        // the simulation is busy at 999 Hz, to show the error
        if config.rate.get() == 999 {
            return RpcResult::Ready(Err(RateError::Busy));
        }
        self.shared.lock().unwrap().rate = config.rate.get();
        self.mode = config.mode;
        RpcResult::Ready(Ok(()))
    }

    async fn read_once(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Sample> {
        RpcResult::Ready(self.sample(0))
    }

    async fn set_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Mode,
    ) -> SetResult<()> {
        self.mode = value;
        SetResult::Set
    }

    async fn get_mode(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<Mode, ()> {
        GetResult::Value(self.mode)
    }

    async fn set_gain(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Fixed<Ratio, Centi, u16, Range<100, 1600>>,
    ) -> SetResult<()> {
        self.gain = value;
        // observers with `OnChange` get it at the next poll: the generated code marks it changed (SRV-57)
        SetResult::Set
    }

    async fn get_gain(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Fixed<Ratio, Centi, u16, Range<100, 1600>>, ()> {
        GetResult::Value(self.gain)
    }

    async fn samples(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
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

    async fn write_commands(&mut self, cx: &mut Context<'_, impl EventOut>, value: Command) {
        match value {
            Command::Trigger => {
                let sample = self.sample(0);
                api_impl::stream_data_ser()
                    .samples_send(&sample, cx)
                    .await
                    .unwrap();
            }
            Command::Unknown(_) => {}
            Command::Mark { seq } => self.seq = seq.get(),
        }
    }

    fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..4)
    }

    async fn calibrate(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        offset: Fixed<Kelvin, Milli, i32>,
    ) -> RpcResult<()> {
        self.offset = offset.get();
        RpcResult::Ready(())
    }

    async fn get_uptime(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Fixed<Second, One, u32>, ()> {
        GetResult::Value(Fixed::new(42))
    }

    /// The sensor reads open while channel 0 is disabled.
    async fn self_test(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Result<(), SelfTestError>> {
        RpcResult::Ready(if self.enabled[0] {
            Ok(())
        } else {
            Err(SelfTestError::SensorOpen)
        })
    }

    async fn set_channel_enabled(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [UNib32; 1],
        value: bool,
    ) -> SetResult<()> {
        self.enabled[index_chain[0].0 as usize] = value;
        SetResult::Set
    }

    async fn get_channel_enabled(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [UNib32; 1],
    ) -> GetResult<bool, ()> {
        GetResult::Value(self.enabled[index_chain[0].0 as usize])
    }

    async fn channel_last(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [UNib32; 1],
    ) -> RpcResult<Sample> {
        RpcResult::Ready(self.sample(index_chain[0].0 as u8))
    }

    async fn set_indication_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: ww_indication::Mode,
    ) -> SetResult<()> {
        self.indication = value;
        SetResult::Set
    }

    async fn get_indication_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<ww_indication::Mode, ()> {
        GetResult::Value(self.indication)
    }

    fn observe_state(&mut self) -> &mut api_impl::ObserveState {
        &mut self.observe
    }
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        thermo_api_w2 :: Thermo for super::Server,
        server = true, no_alloc = true, use_async = true,
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
        omit = ["indication.brightness", "indication.identify", "indication.NAMES"],
        debug_to_file = "../../target/thermo_w2_server.rs",
    );
}

impl tests_common::TestProcessEventsAsync for Server {
    type Medium = ();

    async fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl EventOut,
        medium: (),
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.shared.lock().unwrap().requests += 1;
        self.process_request_bytes(bytes, scratch, out, medium)
            .await
    }

    async fn poll(&mut self, out: &mut impl EventOut, now_ms: u64) {
        let mut buf = core::mem::take(&mut self.buf);
        _ = self.poll_properties(out, (), now_ms * 1000, &mut buf).await;
        self.buf = buf;
    }
}

/// The firmware saying `wire` in link setup: its own (`THERMO_WIRE_VERSION`), or another line to test refusals.
pub fn start(path: &str, wire: WireVersion) -> (Shared, tests_common::TestDevice) {
    let shared = Shared::default();
    let server = Server {
        observe: api_impl::ObserveState::new(Default::default()),
        shared: shared.clone(),
        mode: Mode::Idle,
        gain: Fixed::try_new(100).unwrap(),
        offset: 0,
        seq: 0,
        enabled: [true; 4],
        indication: ww_indication::Mode::Normal,
        buf: vec![0; 512],
    };
    let device = tests_common::start_device_async(
        path,
        server,
        thermo_api_w2::THERMO_FULL_GID,
        wire,
        api_impl::api_hash(),
    );
    (shared, device)
}
