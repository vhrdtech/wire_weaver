//! The sync server (`use_async = false`): one handler per resource kind of `DispatchGoldenTest`, deterministic so
//! its reply bytes can be frozen. See [crate::async_server] for the `use_async = true` twin: both must answer the
//! same requests with the same bytes (TEST-31).

use dispatch_golden_api::GuardedError;
use wire_weaver::prelude::*;
use ww_user_info::{UserInfoCaps, UserInfoError};

#[derive(Default)]
pub struct Server {
    pub plain: u8,
    pub guarded: u8,
    pub channel_level: [u8; 2],
    pub info_name: String,
    pub info_notes: String,
    /// Who to answer when `answer_deferred` runs, set by `deferred`.
    pub deferred_reply: Option<ReplyTo>,
}

impl Server {
    fn no_args(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Ready(())
    }

    fn with_args(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        a: u8,
        b: u16,
    ) -> RpcResult<u32> {
        Ready(a as u32 + b as u32 * 1000)
    }

    fn absent(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Unimplemented.into()
    }

    fn deferred(&mut self, cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<u8> {
        self.deferred_reply = cx.reply_to();
        Deferred
    }

    fn answer_deferred(
        &mut self,
        cx: &mut Context<'_, impl BlockingEventOut>,
        value: u8,
    ) -> RpcResult<()> {
        if let Some(reply_to) = self.deferred_reply.take() {
            Self::deferred_send_return_blocking(cx, reply_to, value).unwrap();
        }
        Ready(())
    }

    fn set_plain(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: u8,
    ) -> SetResult<()> {
        self.plain = value;
        Set
    }

    fn get_plain(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<u8, ()> {
        Value(self.plain)
    }

    fn set_guarded(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: u8,
    ) -> SetResult<GuardedError> {
        if value > 100 {
            SetError(GuardedError::TooBig)
        } else {
            self.guarded = value;
            Set
        }
    }

    fn get_guarded(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<u8, GuardedError> {
        Value(self.guarded)
    }

    fn samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Ready(())
    }

    fn close_samples(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_samples(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }

    fn sideband_commands(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }

    fn write_commands(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, _value: u8) {}

    fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..2)
    }

    fn channel_pulse(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [UNib32; 1],
    ) -> RpcResult<u8> {
        Ready(index_chain[0].0 as u8)
    }

    fn set_channel_level(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [UNib32; 1],
        value: u8,
    ) -> SetResult<()> {
        self.channel_level[index_chain[0].0 as usize] = value;
        Set
    }

    fn get_channel_level(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        index_chain: [UNib32; 1],
    ) -> GetResult<u8, ()> {
        Value(self.channel_level[index_chain[0].0 as usize])
    }

    fn set_info_name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: MaxLen<&'_ str, 32>,
    ) -> SetResult<UserInfoError<'_>> {
        self.info_name = value.to_string();
        Set
    }

    fn get_info_name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<MaxLen<&'_ str, 32>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked(&self.info_name))
    }

    fn set_info_notes(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        value: MaxLen<&'_ str, 256>,
    ) -> SetResult<UserInfoError<'_>> {
        self.info_notes = value.to_string();
        Set
    }

    fn get_info_notes(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<MaxLen<&'_ str, 256>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked(&self.info_notes))
    }
}

impl api_impl::DispatchGoldenTestConsts for Server {
    const LIMIT: u16 = 1000;
    const INFO_CAPABILITIES: UserInfoCaps = UserInfoCaps {
        name_max: 32,
        notes_max: 256,
        persistent: false,
    };
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        dispatch_golden_api :: DispatchGoldenTest for super::Server,
        server = true, no_alloc = true, use_async = false, multi_req = true,
        method_model = "deferred=deferred, _=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
        // the device resolves global paths itself, by the traits' hashes (SRV-53), as without introspection
        global_paths = "full",
    );
}

impl tests_common::TestProcessEvents for Server {
    type Medium = ();

    fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl BlockingEventOut,
        medium: (),
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(bytes, scratch, out, medium)
    }
}
