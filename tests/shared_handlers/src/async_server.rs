//! An async allocating server that shares everything but `bump`, `sensors.calibrate` and `motor`, with sync and
//! async handlers mixed (`handler_model`): the same shared set as [crate::blocking::Server]. `channel[i].scaled(0)`
//! waits until another shared call opens its gate, so it only finishes when two shared calls run side by side.

use std::sync::Mutex;

use shared_handlers_api::FlowKey;
use tokio::sync::Notify;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::{Error as ShrinkWrapError, MaxLen};
use ww_client_server::ResourceIndex;

/// Values the handlers answer with, as in [crate::blocking::Server].
pub struct AsyncServer {
    pub counter: u32,
    pub temp: i32,
    pub speed: u32,
    pub levels: [u32; 3],
    pub flows: [(FlowKey, u32); 2],
    /// Where the deferred `add_later` answers, kept by a `&self` handler.
    pub add_later: Mutex<Option<(ReplyTo, u32)>>,
    /// Opened by `channel[i].scaled(by)` with `by > 0` for a `scaled(0)` waiting on it.
    pub gate: Notify,
    /// The call context the last `add` saw.
    pub add_call: Mutex<Option<CallContext>>,
}

impl Default for AsyncServer {
    fn default() -> Self {
        AsyncServer {
            counter: 4,
            temp: -12,
            speed: 100,
            levels: [10, 11, 12],
            flows: [(FlowKey(10), 5), (FlowKey(20), 7)],
            add_later: Mutex::new(None),
            gate: Notify::new(),
            add_call: Mutex::new(None),
        }
    }
}

fn index(index_chain: [ResourceIndex; 1]) -> usize {
    index_chain[0].get().0 as usize
}

impl AsyncServer {
    fn add(&self, cx: &mut Context<'_, impl EventOut>, a: u32, b: u32) -> RpcResult<u32> {
        *crate::locked(&self.add_call) = Some(*cx.call());
        Ready(a + b)
    }

    async fn add_later(
        &self,
        cx: &mut Context<'_, impl EventOut>,
        a: u32,
        b: u32,
    ) -> RpcResult<u32> {
        let reply_to = cx.reply_to().expect("a reply is expected");
        *crate::locked(&self.add_later) = Some((reply_to, a + b));
        Deferred
    }

    async fn bump(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.counter += 1;
        Ready(())
    }

    fn get_status(&self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u32, CommonError<'_>> {
        Value(7)
    }

    async fn set_counter(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: u32,
    ) -> SetResult<CommonError<'_>> {
        self.counter = value;
        Set
    }

    async fn get_counter(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.counter)
    }

    async fn events(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }

    async fn close_events(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

    async fn sideband_events(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }

    fn valid_indices_root_channel(&self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..3)
    }

    fn valid_indices_root_flow(&self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..2)
    }

    async fn flow_index_of(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
        key: FlowKey,
    ) -> RpcResult<Option<UNib32>> {
        Ready(
            self.flows
                .iter()
                .position(|(k, _)| *k == key)
                .map(|i| UNib32(i as u32)),
        )
    }

    async fn get_flow_key(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ResourceIndex; 1],
    ) -> GetResult<FlowKey, CommonError<'_>> {
        Value(self.flows[index(index_chain)].0)
    }

    async fn get_sensors_temp(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<i32, CommonError<'_>> {
        Value(self.temp)
    }

    async fn sensors_calibrate(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        offset: i32,
    ) -> RpcResult<i32> {
        self.temp += offset;
        Ready(self.temp)
    }

    async fn set_motor_speed(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: u32,
    ) -> SetResult<CommonError<'_>> {
        self.speed = value;
        Set
    }

    async fn get_motor_speed(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.speed)
    }

    async fn get_channel_level(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ResourceIndex; 1],
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.levels[index(index_chain)])
    }

    async fn channel_scaled(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ResourceIndex; 1],
        by: u32,
    ) -> RpcResult<u32> {
        if by == 0 {
            self.gate.notified().await;
        } else {
            self.gate.notify_waiters();
        }
        Ready(self.levels[index(index_chain)] * by)
    }

    async fn get_flow_packets(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [ResourceIndex; 1],
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.flows[index(index_chain)].1)
    }

    async fn set_info_name(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _value: MaxLen<String, 32>,
    ) -> SetResult<CommonError<'_>> {
        Set
    }

    async fn get_info_name(
        &self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MaxLen<String, 32>, CommonError<'_>> {
        Value(MaxLen::new("dev".to_owned()).expect("short"))
    }
}

impl api_impl::SharedHandlersConsts for AsyncServer {
    const INFO_CAPABILITIES: u8 = 3;
}

/// What a host runtime calls (SRV-12): the exclusive and the shared entry, from the connection's origin.
impl WireWeaverAsyncApiBackend for AsyncServer {
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

    fn version(&self) -> FullVersion<'_> {
        shared_handlers_api::SHARED_HANDLERS_FULL_GID
    }
}

impl WireWeaverSharedApiBackend for AsyncServer {
    fn is_shared(data: &[u8]) -> bool {
        AsyncServer::is_shared(data)
    }

    async fn process_shared_bytes<'a>(
        &self,
        origin: &CallContext,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_shared_from(origin, data, scratch, out, medium)
            .await
    }
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        shared_handlers_api :: SharedHandlers for super::AsyncServer,
        server = true, no_alloc = false, use_async = true, multi_req = true,
        handler_model = "add|status=sync, _=async",
        receiver_model = "bump|sensors.calibrate|motor.*=exclusive, _=shared",
        method_model = "add_later=deferred, _=immediate",
        property_model = "_=get_set",
        global_paths = "full",
    );
}
