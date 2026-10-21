//! `ww_dfu` end to end: a fake A/B device with its inactive slot in RAM, driven by the std client through every
//! trait. Checks that the traits go through codegen (fixed arrays, enums with data, borrowed vectors) and that a
//! step-by-step session and a bulk start behave as the docs say. The protocol logic itself (plan parsing, hashing,
//! signatures) belongs to the implementation, chunky; here a chunk "hash" is just its first 8 bytes.

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};
    use wire_weaver::prelude::*;
    use ww_dfu::{
        BootState, BootTarget, BulkStatus, ChunkState, Error, LastResult, NodeInfo, Offers,
        Outcome, PlanAck, Profile, Progress, Role, Session, Slot, SlotLayout, SlotSwitch,
        StageLimits, Target,
    };

    const CHUNK: usize = 16;
    const CHUNKS: usize = 4;
    const MAX_PIECE: usize = 8;

    /// What the fake device holds and what the test inspects afterwards.
    struct Device {
        active: [u8; CHUNK * CHUNKS],
        inactive: [u8; CHUNK * CHUNKS],
        boot_state: BootState,
        /// Steps in the session's plan, `None` without a session.
        steps: Option<u32>,
        next_step: u32,
        chunk_offset: u32,
        plan: Vec<u8>,
        rebooted_into: Option<BootTarget>,
    }

    impl Default for Device {
        fn default() -> Self {
            let mut active = [0u8; CHUNK * CHUNKS];
            for (i, b) in active.iter_mut().enumerate() {
                *b = i as u8;
            }
            Device {
                active,
                inactive: [0xFF; CHUNK * CHUNKS],
                boot_state: BootState::Confirmed,
                steps: None,
                next_step: 0,
                chunk_offset: 0,
                plan: Vec::new(),
                rebooted_into: None,
            }
        }
    }

    fn chunk_hash(chunk: &[u8]) -> [u8; 8] {
        chunk[..8].try_into().unwrap()
    }

    impl Device {
        /// The step-order rule of `Stage`: ahead of its turn is refused, behind it is a resend and skipped.
        fn check_order(&self, index: u32) -> Result<bool, Error> {
            let Some(steps) = self.steps else {
                return Err(Error::NoSession);
            };
            if index >= steps {
                return Err(Error::OutOfRange);
            }
            if index > self.next_step {
                return Err(Error::OutOfOrder(self.next_step));
            }
            Ok(index < self.next_step)
        }
    }

    /// Largest `StageBulk::plan` segment in bytes, known from the `MaxLen` bounds
    const PLAN_SEGMENT_MAX: u32 =
        server::api_impl::WwDfuStageBulkStreamSerializer::PLAN_MAX_ITEM_BYTES.unwrap();

    mod server {
        use super::*;

        pub struct DfuServer {
            pub dev: Arc<RwLock<Device>>,
            /// Room for two of the largest segments, freed as soon as a segment is written
            pub plan_credit: ww_stream::sink::SinkCredit,
        }

        impl DfuServer {
            fn node_node(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<NodeInfo<'_>> {
                let dev = self.dev.read().unwrap();
                Ready(NodeInfo {
                    role: Role::App,
                    profile: Profile::AB(SlotSwitch::Copy),
                    product: MaxLen::new("dfu-test").unwrap(),
                    geometry_id: 0x1234_5678,
                    product_id: 1,
                    seal: ww_dfu::Seal::Ed25519,
                    app_version: Some(7),
                    version_floor: 5,
                    manifest_digest: Some([1, 2, 3, 4, 5, 6, 7, 8]),
                    boot_state: dev.boot_state,
                    last_result: LastResult {
                        outcome: Outcome::Nothing,
                        version: 0,
                    },
                    offers: Offers {
                        boot_control: true,
                        inventory: true,
                        stage: true,
                        stage_bulk: true,
                    },
                })
            }

            fn boot_boot_state(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<BootState> {
                Ready(self.dev.read().unwrap().boot_state)
            }

            fn boot_confirm(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), Error>> {
                let mut dev = self.dev.write().unwrap();
                if dev.boot_state == BootState::Trial {
                    dev.boot_state = BootState::Confirmed;
                }
                Ready(Ok(()))
            }

            fn boot_revert(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<(), Error>> {
                let mut dev = self.dev.write().unwrap();
                match dev.boot_state {
                    BootState::Trial | BootState::Staged => {
                        dev.boot_state = BootState::Reverted;
                        Ready(Ok(()))
                    }
                    state => Ready(Err(Error::WrongState(state))),
                }
            }

            fn boot_reboot(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                into: BootTarget,
            ) -> RpcResult<Result<(), Error>> {
                self.dev.write().unwrap().rebooted_into = Some(into);
                Ready(Ok(()))
            }

            fn inventory_layout(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _slot: Slot,
            ) -> RpcResult<Result<SlotLayout, Error>> {
                Ready(Ok(SlotLayout {
                    chunk_size: CHUNK as u32,
                    chunk_count: CHUNKS as u16,
                    erase_size: (CHUNK * 2) as u32,
                    erase_count: (CHUNKS / 2) as u16,
                }))
            }

            fn inventory_manifest_digest(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _slot: Slot,
            ) -> RpcResult<Result<[u8; 8], Error>> {
                Ready(Ok([1, 2, 3, 4, 5, 6, 7, 8]))
            }

            fn inventory_chunks(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                slot: Slot,
                first: u16,
                count: u16,
            ) -> RpcResult<Result<MaxLen<RefVec<'_, ChunkState>, { ww_dfu::MAX_CHUNKS_PAGE }>, Error>>
            {
                let dev = self.dev.read().unwrap();
                let flash = match slot {
                    Slot::Active => &dev.active,
                    Slot::Inactive => &dev.inactive,
                };
                if first as usize >= CHUNKS {
                    return Ready(Err(Error::OutOfRange));
                }
                let end =
                    (first as usize + (count as usize).min(ww_dfu::MAX_CHUNKS_PAGE)).min(CHUNKS);
                let states: Vec<ChunkState> = flash[first as usize * CHUNK..end * CHUNK]
                    .chunks(CHUNK)
                    .map(|c| {
                        if c.iter().all(|b| *b == 0xFF) {
                            ChunkState::Blank
                        } else {
                            ChunkState::Hash(chunk_hash(c))
                        }
                    })
                    .collect();
                // A real no_alloc server builds the reply in its own buffer; a leaked Vec stands in for it here.
                Ready(Ok(MaxLen::new(RefVec::Slice {
                    slice: Box::leak(states.into_boxed_slice()),
                })
                .unwrap()))
            }

            fn stage_limits(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<StageLimits> {
                Ready(StageLimits {
                    max_piece: MAX_PIECE as u32,
                    in_flight: 1,
                    live: true,
                    stored: false,
                    plan_area: 0,
                    writes_in_background: true,
                })
            }

            fn stage_begin(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                target: Target,
                header: MaxLen<RefVec<'_, u8>, { ww_dfu::MAX_HEADER_LEN }>,
            ) -> RpcResult<Result<Session, Error>> {
                let mut dev = self.dev.write().unwrap();
                if target != Target::Live {
                    return Ready(Err(Error::Unsupported));
                }
                if !dev.boot_state.may_stage() {
                    return Ready(Err(Error::WrongState(dev.boot_state)));
                }
                // Test header: one byte, the step count.
                let Some(Ok(steps)) = header.iter().next() else {
                    return Ready(Err(Error::BadHeader));
                };
                let steps = steps as u32;
                if dev.steps == Some(steps) {
                    return Ready(Ok(Session {
                        steps,
                        resumed: true,
                        resume_from: dev.next_step,
                    }));
                }
                dev.steps = Some(steps);
                dev.next_step = 0;
                dev.chunk_offset = 0;
                Ready(Ok(Session {
                    steps,
                    resumed: false,
                    resume_from: 0,
                }))
            }

            fn stage_erase(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: u32,
                block: u16,
            ) -> RpcResult<Result<(), Error>> {
                let mut dev = self.dev.write().unwrap();
                match dev.check_order(index) {
                    Err(e) => return Ready(Err(e)),
                    Ok(true) => return Ready(Ok(())),
                    Ok(false) => {}
                }
                let start = block as usize * CHUNK * 2;
                if start + CHUNK * 2 > dev.inactive.len() {
                    return Ready(Err(Error::OutOfRange));
                }
                dev.inactive[start..start + CHUNK * 2].fill(0xFF);
                dev.next_step += 1;
                Ready(Ok(()))
            }

            fn stage_copy(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                index: u32,
                dst: u16,
                src: u16,
                hash: [u8; 8],
            ) -> RpcResult<Result<(), Error>> {
                let mut dev = self.dev.write().unwrap();
                match dev.check_order(index) {
                    Err(e) => return Ready(Err(e)),
                    Ok(true) => return Ready(Ok(())),
                    Ok(false) => {}
                }
                let (dst, src) = (dst as usize, src as usize);
                if dst >= CHUNKS || src >= CHUNKS {
                    return Ready(Err(Error::OutOfRange));
                }
                let source: [u8; CHUNK] = dev.active[src * CHUNK..(src + 1) * CHUNK]
                    .try_into()
                    .unwrap();
                if chunk_hash(&source) != hash {
                    return Ready(Err(Error::SourceMismatch));
                }
                dev.inactive[dst * CHUNK..(dst + 1) * CHUNK].copy_from_slice(&source);
                dev.next_step += 1;
                Ready(Ok(()))
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
                let mut dev = self.dev.write().unwrap();
                match dev.check_order(index) {
                    Err(e) => return Ready(Err(e)),
                    Ok(true) => return Ready(Ok(())),
                    Ok(false) => {}
                }
                let data: Vec<u8> = data.iter().map(|b| b.unwrap()).collect();
                if data.len() > MAX_PIECE {
                    return Ready(Err(Error::PieceTooBig));
                }
                let (dst, offset) = (dst as usize, offset as usize);
                if dst >= CHUNKS || offset + data.len() > CHUNK || offset as u32 != dev.chunk_offset
                {
                    return Ready(Err(Error::OutOfRange));
                }
                let at = dst * CHUNK + offset;
                dev.inactive[at..at + data.len()].copy_from_slice(&data);
                dev.chunk_offset += data.len() as u32;
                if dev.chunk_offset as usize == CHUNK {
                    dev.chunk_offset = 0;
                    if chunk_hash(&dev.inactive[dst * CHUNK..]) != hash {
                        return Ready(Err(Error::HashMismatch));
                    }
                    dev.next_step += 1;
                }
                Ready(Ok(()))
            }

            fn stage_progress(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<Progress, Error>> {
                let dev = self.dev.read().unwrap();
                let Some(steps) = dev.steps else {
                    return Ready(Err(Error::NoSession));
                };
                Ready(Ok(Progress {
                    steps,
                    next_step: dev.next_step,
                    chunk_offset: dev.chunk_offset,
                    plan_bytes: dev.plan.len() as u32,
                }))
            }

            fn stage_finish(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<Result<BootState, Error>> {
                let mut dev = self.dev.write().unwrap();
                let Some(steps) = dev.steps else {
                    return Ready(Err(Error::NoSession));
                };
                if dev.next_step < steps {
                    return Ready(Err(Error::Incomplete));
                }
                dev.steps = None;
                dev.boot_state = BootState::Staged;
                Ready(Ok(BootState::Staged))
            }

            fn stage_abort(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                self.dev.write().unwrap().steps = None;
                Ready(())
            }

            fn stage_event(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_stage_event(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_stage_event(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn bulk_begin_bulk(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _target: Target,
                _header: MaxLen<RefVec<'_, u8>, { ww_dfu::MAX_HEADER_LEN }>,
                _total_len: u32,
                _ack_every: u32,
            ) -> RpcResult<Result<Session, Error>> {
                self.dev.write().unwrap().plan.clear();
                Ready(Ok(Session {
                    steps: 0,
                    resumed: false,
                    resume_from: 0,
                }))
            }

            fn credit_bulk_plan(&mut self) -> &mut ww_stream::sink::SinkCredit {
                &mut self.plan_credit
            }

            fn sideband_bulk_plan(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }

            fn write_bulk_plan(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                value: ww_dfu::PlanSegment<'_>,
            ) {
                let mut dev = self.dev.write().unwrap();
                // Segments arrive in order (the sink's flow control), but an offset that doesn't continue the plan is
                // dropped (a real node would acknowledge BulkStatus::Gap on plan_ack).
                if value.offset as usize == dev.plan.len() {
                    dev.plan.extend(value.data.iter().map(|b| b.unwrap()));
                }
                // credit counts the segment's bytes as sent
                self.plan_credit.free(self.plan_credit.last_item_len());
            }

            fn bulk_plan_ack(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
            ) -> RpcResult<()> {
                Ready(())
            }

            fn close_bulk_plan_ack(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

            fn sideband_bulk_plan_ack(
                &mut self,
                _cx: &mut Context<'_, impl BlockingEventOut>,
                _sideband: ww_client_server::StreamSideband,
            ) -> Option<ww_client_server::StreamSideband> {
                None
            }
        }

        pub mod api_impl {
            wire_weaver::ww_codegen!(
                dfu_api :: DfuDevice for super::DfuServer,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
            );
        }

        impl tests_common::TestProcessEvents for DfuServer {
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
    }

    mod client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct DfuClient {
            pub cmd: Commander,
        }

        impl WwClient for DfuClient {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn from_cmd(cmd: Commander) -> Self {
                Self { cmd }
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                dfu_api :: DfuDevice for super::DfuClient,
                client = "std_client",
            );
        }
    }

    async fn connect(
        name: &str,
    ) -> (
        client::DfuClient,
        Arc<RwLock<Device>>,
        tests_common::TestDevice,
    ) {
        let _ = tracing_subscriber::fmt::try_init();
        let dev = Arc::new(RwLock::new(Device::default()));
        let server = server::DfuServer {
            dev: dev.clone(),
            plan_credit: ww_stream::sink::SinkCredit::sized_for(
                2 * PLAN_SEGMENT_MAX,
                Some(PLAN_SEGMENT_MAX),
            ),
        };
        let device = tests_common::start_device(
            name,
            server,
            dfu_api::DFU_DEVICE_FULL_GID,
            server::api_impl::api_hash(),
        );
        let client = client::DfuClient::config(|c| c.in_process_path(device.path()))
            .connect()
            .await
            .expect("connect");
        (client, dev, device)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn node_and_inventory() {
        let (client, _dev, _device) = connect("dfu/inventory").await;

        let node = client.node().node().call().await.unwrap();
        assert_eq!(node.profile, Profile::AB(SlotSwitch::Copy));
        assert_eq!(&*node.product, "dfu-test");
        assert_eq!(node.manifest_digest, Some([1, 2, 3, 4, 5, 6, 7, 8]));
        assert!(node.offers.stage && node.offers.stage_bulk);

        let layout = client
            .inventory()
            .layout(Slot::Active)
            .call()
            .await
            .unwrap();
        assert_eq!(layout.chunk_count, CHUNKS as u16);

        let digest = client
            .inventory()
            .manifest_digest(Slot::Active)
            .call()
            .await
            .unwrap();
        assert_eq!(digest, [1, 2, 3, 4, 5, 6, 7, 8]);

        let chunks = client
            .inventory()
            .chunks(Slot::Active, 1, 2)
            .call()
            .await
            .unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks[0],
            ChunkState::Hash([16, 17, 18, 19, 20, 21, 22, 23])
        );
        let blank = client
            .inventory()
            .chunks(Slot::Inactive, 0, 9)
            .call()
            .await
            .unwrap();
        assert_eq!(blank.len(), CHUNKS, "a page stops at the end of the slot");
        assert!(blank.iter().all(|c| *c == ChunkState::Blank));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn step_by_step_session() {
        let (client, dev, _device) = connect("dfu/steps").await;
        let mut stage = client.stage();

        let limits = stage.limits().call().await.unwrap();
        assert_eq!(limits.max_piece, MAX_PIECE as u32);
        assert_eq!(
            stage.progress().call().await.unwrap_err().device(),
            Some(&Error::NoSession)
        );

        // Plan: erase block 0, copy active chunk 2 to chunk 0, write chunk 1 in two pieces.
        let session = stage
            .begin(Target::Live, MaxLen::new(vec![3]).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!((session.steps, session.resumed), (3, false));
        assert_eq!(
            stage
                .copy(1, 0, 2, [0; 8])
                .call()
                .await
                .unwrap_err()
                .device(),
            Some(&Error::OutOfOrder(0))
        );

        stage.erase(0, 0).call().await.unwrap();
        stage.erase(0, 0).call().await.unwrap(); // a resend is skipped
        assert_eq!(
            stage
                .copy(1, 0, 2, [0; 8])
                .call()
                .await
                .unwrap_err()
                .device(),
            Some(&Error::SourceMismatch)
        );
        let src_hash = [32, 33, 34, 35, 36, 37, 38, 39];
        stage.copy(1, 0, 2, src_hash).call().await.unwrap();
        // a resend of a done step
        stage.copy(1, 0, 2, src_hash).call().await.unwrap();

        // Resume: begin with the same header answers where to continue.
        let again = stage
            .begin(Target::Live, MaxLen::new(vec![3]).unwrap())
            .call()
            .await
            .unwrap();
        assert_eq!((again.resumed, again.resume_from), (true, 2));

        let data: Vec<u8> = (100..100 + CHUNK as u8).collect();
        let hash: [u8; 8] = data[..8].try_into().unwrap();
        assert_eq!(
            stage
                .write(2, 1, hash, 0, MaxLen::new(vec![0; MAX_PIECE + 1]).unwrap())
                .call()
                .await
                .unwrap_err()
                .device(),
            Some(&Error::PieceTooBig)
        );
        // Past the protocol-wide bound the client refuses to send at all; nothing reaches the device.
        let too_long = MaxLen::new_unchecked(vec![0; ww_dfu::MAX_PIECE_LEN + 1]);
        assert!(stage.write(2, 1, hash, 0, too_long).call().await.is_err());
        assert_eq!(stage.progress().call().await.unwrap().chunk_offset, 0);
        stage
            .write(2, 1, hash, 0, MaxLen::new(data[..8].to_vec()).unwrap())
            .call()
            .await
            .unwrap();
        let progress = stage.progress().call().await.unwrap();
        assert_eq!((progress.next_step, progress.chunk_offset), (2, 8));
        stage
            .write(2, 1, hash, 8, MaxLen::new(data[8..].to_vec()).unwrap())
            .call()
            .await
            .unwrap();

        assert_eq!(stage.finish().call().await.unwrap(), BootState::Staged);
        {
            let dev = dev.read().unwrap();
            assert_eq!(&dev.inactive[..CHUNK], &dev.active[2 * CHUNK..3 * CHUNK]);
            assert_eq!(&dev.inactive[CHUNK..2 * CHUNK], &data[..]);
        }

        let mut boot = client.boot();
        assert_eq!(boot.boot_state().call().await.unwrap(), BootState::Staged);
        boot.reboot(BootTarget::App).call().await.unwrap();
        assert_eq!(dev.read().unwrap().rebooted_into, Some(BootTarget::App));
        dev.write().unwrap().boot_state = BootState::Trial;
        boot.confirm().call().await.unwrap();
        assert_eq!(
            boot.boot_state().call().await.unwrap(),
            BootState::Confirmed
        );
        assert_eq!(
            boot.revert().call().await.unwrap_err().device(),
            Some(&Error::WrongState(BootState::Confirmed))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bulk_segments_in_order() {
        let (client, dev, _device) = connect("dfu/bulk").await;
        let mut bulk = client.bulk();
        let session = bulk
            .begin_bulk(Target::Live, MaxLen::new(vec![0]).unwrap(), 12, 0)
            .call()
            .await
            .unwrap();
        assert_eq!(session.resume_from, 0);

        let mut plan = bulk.plan().await.unwrap();
        assert!(plan.is_credit());
        for (offset, data) in [
            (0, vec![1, 2, 3, 4]),
            (8, vec![9, 10, 11, 12]),
            (4, vec![5, 6, 7, 8]),
        ] {
            // The second one doesn't continue the plan and is dropped by the node.
            plan.send_owned(ww_dfu::PlanSegmentOwned {
                offset,
                data: MaxLen::new(data).unwrap(),
            })
            .await
            .unwrap();
        }
        // a credit sink: flush() returns once the node accepted every segment
        plan.flush().await.unwrap();
        // one unit per segment
        assert_eq!(plan.credit_progress(), Some((3, 3)));
        // MAX_PIECE_LEN of data, the offset and the encoding around them
        assert_eq!(PLAN_SEGMENT_MAX, 1031);
        assert_eq!(dev.read().unwrap().plan, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            client.stage().progress().call().await.unwrap_err().device(),
            Some(&Error::NoSession)
        );

        // The ack type round-trips with an error inside a status.
        let ack = PlanAck {
            have: 8,
            status: BulkStatus::Failed(Error::OutOfOrder(3)),
        };
        let mut buf = [0u8; 64];
        let bytes = ack.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(PlanAck::from_ww_bytes(bytes).unwrap(), ack);
    }
}
