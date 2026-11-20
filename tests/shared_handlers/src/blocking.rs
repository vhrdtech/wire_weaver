//! A blocking `no_alloc` server whose shared items are named one by one: the handlers `ww api scaffold
//! --receiver-model ..` writes, `&self` for the shared items and the arrays they sit in.

use std::sync::Mutex;

use shared_handlers_api::FlowKey;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::MaxLen;
use ww_client_server::ResourceIndex;

/// The receiver model of [Server], the shared set every test case expects.
pub const RECEIVER_MODEL: &str =
    "add|add_later|status|counter|sensors.temp|channel.*|flow.packets|flow_keys.*|info.name=shared";

/// Values the handlers answer with; nothing changes them but the exclusive handlers.
pub struct Server {
    pub counter: u32,
    pub temp: i32,
    pub speed: u32,
    pub levels: [u32; 3],
    pub flows: [(FlowKey, u32); 2],
    /// Where the deferred `add_later` answers, kept by a `&self` handler.
    pub add_later: Mutex<Option<(ReplyTo, u32)>>,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            counter: 4,
            temp: -12,
            speed: 100,
            levels: [10, 11, 12],
            flows: [(FlowKey(10), 5), (FlowKey(20), 7)],
            add_later: Mutex::new(None),
        }
    }
}

fn index(index_chain: [ResourceIndex; 1]) -> usize {
    index_chain[0].get().0 as usize
}

impl Server {
    fn add(&self, _cx: &mut Context<'_, impl BlockingEventOut>, a: u32, b: u32) -> RpcResult<u32> {
        Ready(a + b)
    }

    fn add_later(
        &self,
        cx: &mut Context<'_, impl BlockingEventOut>,
        a: u32,
        b: u32,
    ) -> RpcResult<u32> {
        let reply_to = cx.reply_to().expect("a reply is expected");
        *crate::locked(&self.add_later) = Some((reply_to, a + b));
        Deferred
    }

    fn bump(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        self.counter += 1;
        Ready(())
    }

    fn get_status(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<u32, CommonError<'_>> {
        Value(7)
    }

    fn set_counter(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: u32,
    ) -> SetResult<CommonError<'_>> {
        self.counter = value;
        Set
    }

    fn get_counter(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.counter)
    }

    fn events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Ready(())
    }

    fn close_events(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_events(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
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

    fn flow_index_of(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        key: FlowKey,
    ) -> RpcResult<Option<UNib32>> {
        Ready(
            self.flows
                .iter()
                .position(|(k, _)| *k == key)
                .map(|i| UNib32(i as u32)),
        )
    }

    fn get_flow_key(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [ResourceIndex; 1],
    ) -> GetResult<FlowKey, CommonError<'_>> {
        Value(self.flows[index(index_chain)].0)
    }

    fn get_sensors_temp(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<i32, CommonError<'_>> {
        Value(self.temp)
    }

    fn sensors_calibrate(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        offset: i32,
    ) -> RpcResult<i32> {
        self.temp += offset;
        Ready(self.temp)
    }

    fn set_motor_speed(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: u32,
    ) -> SetResult<CommonError<'_>> {
        self.speed = value;
        Set
    }

    fn get_motor_speed(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.speed)
    }

    fn get_channel_level(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [ResourceIndex; 1],
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.levels[index(index_chain)])
    }

    fn channel_scaled(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [ResourceIndex; 1],
        by: u32,
    ) -> RpcResult<u32> {
        Ready(self.levels[index(index_chain)] * by)
    }

    fn get_flow_packets(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [ResourceIndex; 1],
    ) -> GetResult<u32, CommonError<'_>> {
        Value(self.flows[index(index_chain)].1)
    }

    fn set_info_name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: MaxLen<&'_ str, 32>,
    ) -> SetResult<CommonError<'_>> {
        Set
    }

    fn get_info_name(
        &self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<MaxLen<&'_ str, 32>, CommonError<'_>> {
        Value(MaxLen::new("dev").expect("short"))
    }
}

impl api_impl::SharedHandlersConsts for Server {
    const INFO_CAPABILITIES: u8 = 3;
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        shared_handlers_api :: SharedHandlers for super::Server,
        server = true, no_alloc = true, use_async = false, multi_req = true,
        receiver_model = "add|add_later|status|counter|sensors.temp|channel.*|flow.packets|flow_keys.*|info.name=shared",
        method_model = "add_later=deferred, _=immediate",
        property_model = "_=get_set",
        global_paths = "full",
    );
}
