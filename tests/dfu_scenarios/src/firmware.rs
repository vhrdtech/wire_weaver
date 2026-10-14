//! The three firmwares of the simulated device, each a generated `ww_dfu` server forwarding to a [Node]: the app with
//! every update trait, the lean app (`UpdateNode` and `BootControl`) and the updater.

use wire_weaver::prelude::*;
use wire_weaver::ww_version::{ApiHashPair, FullVersion};
use ww_client_server::StreamSideband;
use ww_client_server::credit::SinkCredit;
use ww_dfu::{
    BootState, BootTarget, ChunkState, Error, NodeInfo, PlanSegment, Progress, Session, Slot,
    SlotLayout, StageLimits, Target,
};

use crate::node::Node;

/// Handlers of `UpdateNode` and `BootControl`, the same on every firmware.
macro_rules! boot_handlers {
    () => {
        fn node_node(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<NodeInfo<'_>> {
            Ready(self.node.node())
        }

        fn boot_boot_state(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<BootState> {
            Ready(self.node.boot_state())
        }

        fn boot_confirm(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Result<(), Error>> {
            Ready(self.node.confirm())
        }

        fn boot_revert(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Result<(), Error>> {
            Ready(self.node.revert())
        }

        fn boot_reboot(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            into: BootTarget,
        ) -> RpcResult<Result<(), Error>> {
            Ready(self.node.reboot(into))
        }
    };
}

/// Handlers of `Inventory`, `Stage` and `StageBulk`. `$ser` is the generated module with `stream_data_ser()`.
macro_rules! stage_handlers {
    ($ser:path) => {
        fn inventory_layout(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            slot: Slot,
        ) -> RpcResult<Result<SlotLayout, Error>> {
            Ready(self.node.layout(slot))
        }

        fn inventory_manifest_digest(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            slot: Slot,
        ) -> RpcResult<Result<[u8; 8], Error>> {
            Ready(self.node.manifest_digest(slot))
        }

        fn inventory_chunks(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            slot: Slot,
            first: u16,
            count: u16,
        ) -> RpcResult<Result<MaxLen<RefVec<'_, ChunkState>, { ww_dfu::MAX_CHUNKS_PAGE }>, Error>> {
            Ready(
                self.node
                    .chunks(slot, first, count)
                    .map(|page| MaxLen::new(RefVec::Slice { slice: page }).unwrap()),
            )
        }

        fn stage_limits(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<StageLimits> {
            Ready(self.node.limits)
        }

        fn stage_begin(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            target: Target,
            header: MaxLen<RefVec<'_, u8>, { ww_dfu::MAX_HEADER_LEN }>,
        ) -> RpcResult<Result<Session, Error>> {
            let header = bytes(&header);
            Ready(self.node.begin(target, &header))
        }

        fn stage_erase(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            index: u32,
            block: u16,
        ) -> RpcResult<Result<(), Error>> {
            Ready(self.node.erase(index, block))
        }

        fn stage_copy(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            index: u32,
            dst: u16,
            src: u16,
            hash: [u8; 8],
        ) -> RpcResult<Result<(), Error>> {
            Ready(self.node.copy(index, dst, src, hash))
        }

        fn stage_write(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            index: u32,
            dst: u16,
            hash: [u8; 8],
            offset: u32,
            data: MaxLen<RefVec<'_, u8>, { ww_dfu::MAX_PIECE_LEN }>,
        ) -> RpcResult<Result<(), Error>> {
            let data = bytes(&data);
            Ready(self.node.write(index, dst, hash, offset, &data))
        }

        fn stage_progress(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Result<Progress, Error>> {
            Ready(self.node.progress())
        }

        fn stage_finish(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
        ) -> RpcResult<Result<BootState, Error>> {
            Ready(self.node.finish())
        }

        fn stage_abort(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
            self.node.abort();
            Ready(())
        }

        fn sideband_stage_event(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _sideband: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn bulk_begin_bulk(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            target: Target,
            header: MaxLen<RefVec<'_, u8>, { ww_dfu::MAX_HEADER_LEN }>,
            total_len: u32,
            ack_every: u32,
        ) -> RpcResult<Result<Session, Error>> {
            let header = bytes(&header);
            Ready(self.node.begin_bulk(target, &header, total_len, ack_every))
        }

        fn credit_bulk_plan(&mut self) -> &mut SinkCredit {
            &mut self.plan_credit
        }

        fn sideband_bulk_plan(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _sideband: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }

        fn write_bulk_plan(
            &mut self,
            cx: &mut Context<'_, impl BlockingEventOut>,
            value: PlanSegment<'_>,
        ) {
            self.node.segment(ww_dfu::PlanSegmentOwned {
                offset: value.offset,
                data: MaxLen::new(bytes(&value.data)).unwrap(),
            });
            if self.node.power_cut {
                return;
            }
            // the segment is written: its bytes are free again
            self.plan_credit.free(self.plan_credit.last_item_len());
            use $ser as generated;
            for ack in core::mem::take(&mut self.node.acks) {
                _ = generated::stream_data_ser()
                    .bulk()
                    .plan_ack_send_blocking(&ack, cx);
            }
        }

        fn sideband_bulk_plan_ack(
            &mut self,
            _cx: &mut Context<'_, impl BlockingEventOut>,
            _sideband: StreamSideband,
        ) -> Option<StreamSideband> {
            None
        }
    };
}

fn bytes(v: &RefVec<'_, u8>) -> Vec<u8> {
    v.iter().map(|b| b.unwrap()).collect()
}

/// Receive buffer of the `plan` sink: room for two of the largest segments.
pub fn plan_credit() -> SinkCredit {
    let max = app::api_impl::WwDfuStageBulkStreamSerializer::PLAN_MAX_ITEM_BYTES.unwrap();
    SinkCredit::sized_for(2 * max, Some(max))
}

/// A generated server the device runs: what the simulated hardware needs from it.
pub trait Firmware {
    fn node(&mut self) -> &mut Node;
    fn version(&self) -> FullVersion<'static>;
    fn api_hash(&self) -> ApiHashPair<'static>;
    fn process<'a>(
        &mut self,
        bytes: &[u8],
        scratch: &'a mut [u8],
        out: &mut impl BlockingEventOut,
    ) -> Result<&'a [u8], ShrinkWrapError>;
}

macro_rules! firmware {
    ($m:ident, $server:ident, $api:ident, $gid:ident) => {
        impl Firmware for $m::$server {
            fn node(&mut self) -> &mut Node {
                &mut self.node
            }
            fn version(&self) -> FullVersion<'static> {
                $api::$gid
            }
            fn api_hash(&self) -> ApiHashPair<'static> {
                $m::api_impl::api_hash()
            }
            fn process<'a>(
                &mut self,
                bytes: &[u8],
                scratch: &'a mut [u8],
                out: &mut impl BlockingEventOut,
            ) -> Result<&'a [u8], ShrinkWrapError> {
                self.process_request_bytes(bytes, scratch, out, ())
            }
        }
    };
}

pub mod app {
    use super::*;

    pub struct AppServer {
        pub node: Node,
        pub plan_credit: SinkCredit,
    }

    impl AppServer {
        boot_handlers!();
        stage_handlers!(super::app::api_impl);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            dfu_app_api :: DfuApp for super::AppServer,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }
}

pub mod lean_app {
    use super::*;

    pub struct LeanAppServer {
        pub node: Node,
    }

    impl LeanAppServer {
        boot_handlers!();
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            dfu_lean_app_api :: DfuLeanApp for super::LeanAppServer,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }
}

pub mod updater {
    use super::*;

    pub struct UpdaterServer {
        pub node: Node,
        pub plan_credit: SinkCredit,
    }

    impl UpdaterServer {
        boot_handlers!();
        stage_handlers!(super::updater::api_impl);
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            dfu_updater_api :: DfuUpdater for super::UpdaterServer,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }
}

firmware!(app, AppServer, dfu_app_api, DFU_APP_FULL_GID);
firmware!(
    lean_app,
    LeanAppServer,
    dfu_lean_app_api,
    DFU_LEAN_APP_FULL_GID
);
firmware!(
    updater,
    UpdaterServer,
    dfu_updater_api,
    DFU_UPDATER_FULL_GID
);
