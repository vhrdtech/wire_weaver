//! The async server (`use_async = true`): same behaviour as [crate::server::Server], so the corpus can check that
//! `use_async` doesn't change a single reply byte (TEST-31).

use dispatch_golden_api::GuardedError;
use wire_weaver::prelude::*;
use ww_user_info::{NameMax, NotesMax, UserInfoCaps, UserInfoError};

#[derive(Default)]
pub struct AsyncServer {
    pub plain: u8,
    pub guarded: u8,
    pub channel_level: [u8; 2],
    pub info_name: String,
    pub info_notes: String,
    pub deferred_reply: Option<ReplyTo>,
}

impl AsyncServer {
    async fn no_args(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Ready(())
    }

    async fn with_args(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        a: u8,
        b: u16,
    ) -> RpcResult<u32> {
        Ready(a as u32 + b as u32 * 1000)
    }

    async fn absent(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        Unimplemented.into()
    }

    async fn deferred(&mut self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<u8> {
        self.deferred_reply = cx.reply_to();
        Deferred
    }

    async fn answer_deferred(
        &mut self,
        cx: &mut Context<'_, impl EventOut>,
        value: u8,
    ) -> RpcResult<()> {
        if let Some(reply_to) = self.deferred_reply.take() {
            Self::deferred_send_return(cx, reply_to, value)
                .await
                .unwrap();
        }
        Ready(())
    }

    async fn set_plain(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: u8,
    ) -> SetResult<()> {
        self.plain = value;
        Set
    }

    async fn get_plain(&mut self, _cx: &mut Context<'_, impl EventOut>) -> GetResult<u8, ()> {
        Value(self.plain)
    }

    async fn set_guarded(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: u8,
    ) -> SetResult<GuardedError> {
        if value > 100 {
            SetError(GuardedError::TooBig)
        } else {
            self.guarded = value;
            Set
        }
    }

    async fn get_guarded(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<u8, GuardedError> {
        Value(self.guarded)
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

    async fn write_commands(&mut self, _cx: &mut Context<'_, impl EventOut>, _value: u8) {}

    fn valid_indices_root_channel(&mut self) -> ValidIndices<'_> {
        ValidIndices::range_u32(0..2)
    }

    async fn channel_pulse(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [UNib32; 1],
    ) -> RpcResult<u8> {
        Ready(index_chain[0].0 as u8)
    }

    async fn set_channel_level(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [UNib32; 1],
        value: u8,
    ) -> SetResult<()> {
        self.channel_level[index_chain[0].0 as usize] = value;
        Set
    }

    async fn get_channel_level(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        index_chain: [UNib32; 1],
    ) -> GetResult<u8, ()> {
        Value(self.channel_level[index_chain[0].0 as usize])
    }

    async fn set_info_name(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: MaxLen<&'_ str, 32>,
    ) -> SetResult<UserInfoError<'_>> {
        self.info_name = value.to_string();
        Set
    }

    async fn get_info_name(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MaxLen<&'_ str, 32>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked(&self.info_name))
    }

    async fn set_info_notes(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: MaxLen<&'_ str, 256>,
    ) -> SetResult<UserInfoError<'_>> {
        self.info_notes = value.to_string();
        Set
    }

    async fn get_info_notes(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MaxLen<&'_ str, 256>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked(&self.info_notes))
    }
}

impl api_impl::DispatchGoldenTestConsts for AsyncServer {
    const LIMIT: u16 = 1000;
    const INFO_CAPABILITIES: UserInfoCaps = UserInfoCaps {
        name_max: NameMax::bounded(32),
        notes_max: NotesMax::bounded(256),
        persistent: false,
    };
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        dispatch_golden_api :: DispatchGoldenTest for super::AsyncServer,
        server = true, no_alloc = true, use_async = true, multi_req = true,
        method_model = "deferred=deferred, _=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
        // the device resolves global paths itself, by the traits' hashes (SRV-53), as without introspection
        global_paths = "full",
    );
}

impl tests_common::TestProcessEventsAsync for AsyncServer {
    type Medium = ();

    async fn process_request_bytes<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl EventOut,
        medium: (),
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(bytes, scratch, out, medium)
            .await
    }
}
