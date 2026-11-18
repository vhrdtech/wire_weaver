//! The hub's own API (`ww_hub_api`) on one connection: a generated `no_alloc = false` server on `ww_device::Server`,
//! over the connection whose preamble the hub already answered.

use std::sync::Arc;

use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, warn};
use wire_weaver::prelude::*;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_client::local_socket::device::connection_medium;
use ww_device::LinkEvent;
use ww_hub_api::{
    AdoptErrorOwned, CaptureEntryOwned, CaptureErrorOwned, CaptureId, CaptureOptions,
    ClaimEntryOwned, ClaimErrorOwned, ClientEntryOwned, DeviceEntryOwned, HandoverError,
    HubInfoOwned, HubStatusOwned, MAX_WATCHES, Pressure, RamMax, RecordBatchOwned, ReleaseError,
    SaveInfoOwned, SyncEntryOwned, TakeErrorOwned, WatchId,
};

use crate::Hub;
use crate::captures::{HostWatch, SavePath, WatchCount};

/// Longest message on a hub API connection, both ways: device and client lists of a busy bench fit, and a watch's
/// record batch (`ww_hub_api::MAX_BATCH_BYTES`) with its stream header.
const MAX_MESSAGE_LEN: usize = 64 * 1024;

/// A watch's batch from its task to the connection: the watch's index, its generation (a batch of a closed watch
/// isn't sent on one opened at the same index after), the batch, `None` once the capture ended.
type BatchMsg = (u32, u64, Option<RecordBatchOwned>);

/// Serves the hub API to one client until it leaves.
pub(crate) async fn serve_connection(hub: Arc<Hub>, id: u32, rx: RecvHalf, tx: SendHalf) {
    let (tx, rx, mut ended) = connection_medium(rx, tx, MAX_MESSAGE_LEN);
    let mut scratch = vec![0u8; MAX_MESSAGE_LEN];
    let mut event_scratch = vec![0u8; MAX_MESSAGE_LEN];
    let config = ww_device::LinkConfig::new(
        ww_hub_api::HUB_FULL_GID,
        ww_hub_api::HUB_WIRE_VERSION,
        server::api_impl::api_hash(),
        server::api_impl::image_facts(),
        ww_client_server::COMPACT_VERSION,
    );
    let mut server = ww_device::Server::new(
        config,
        tx,
        rx,
        TokioClock::new(),
        &mut scratch,
        &mut event_scratch,
    );
    // one batch in flight: a client slower than its watches makes their tasks wait, which makes the host's cursors
    // fall behind, never the device (SRV-64)
    let (batch_tx, mut batches) = tokio::sync::mpsc::channel::<BatchMsg>(1);
    let mut backend = server::Server {
        hub: hub.clone(),
        id,
        events_open: false,
        stats_open: false,
        adopting: None,
        watches: (0..MAX_WATCHES).map(|_| None).collect(),
        next_generation: 0,
        batch_tx,
    };
    let mut events = hub.events.subscribe();
    let mut stats = hub.sync_stats.subscribe();
    loop {
        tokio::select! {
            ready = server.wait() => match server.handle(ready, &mut backend).await {
                Some(LinkEvent::Up) => hub.client_up(id, server.link().peer_meta().cloned()),
                Some(LinkEvent::Down(reason)) => {
                    // a connection is served once: the client reconnects with a new one
                    debug!("ww hub: client {id} link down: {reason:?}");
                    break;
                }
                None => {}
            },
            _ = ended.wait() => break,
            event = events.recv() => match event {
                Ok(event) => {
                    let handover = event == ww_hub_api::EventOwned::Handover;
                    if backend.events_open && server.is_up() {
                        let _ = server::api_impl::stream_data_ser()
                            .registry()
                            .events_send(&event, &mut server.sink())
                            .await;
                    }
                    if handover {
                        // the client reconnects to the next hub (CLT-40)
                        server.disconnect(hub.closing_reason()).await;
                        break;
                    }
                }
                // a slow client misses events, it can read the lists again
                Err(RecvError::Lagged(n)) => debug!("ww hub: client {id} missed {n} events"),
                Err(RecvError::Closed) => break,
            },
            Some((w, generation, batch)) = batches.recv() => {
                let slot = &mut backend.watches[w as usize];
                if slot.as_ref().is_none_or(|s| s.generation != generation) {
                    continue;
                }
                match batch {
                    Some(batch) if server.is_up() => {
                        let sent = server::api_impl::stream_data_ser()
                            .captures()
                            .watches(w)
                            .records_send(&batch, &mut server.sink())
                            .await;
                        if let Err(e) = sent {
                            warn!("ww hub: client {id}: watch {w} batch not sent: {e:?}");
                        }
                    }
                    Some(_) => {}
                    // the capture ended
                    None => *slot = None,
                }
            }
            entry = stats.recv() => match entry {
                Ok(entry) if backend.stats_open && server.is_up() => {
                    let _ = server::api_impl::stream_data_ser()
                        .sync()
                        .stats_send(&entry, &mut server.sink())
                        .await;
                }
                Ok(_) => {}
                // the next update of the device brings it up to date
                Err(RecvError::Lagged(n)) => debug!("ww hub: client {id} missed {n} sync stats"),
                Err(RecvError::Closed) => break,
            },
        }
    }
}

mod server {
    use super::*;
    use ww_si::Fixed;

    /// What a watch's batch may take of a message: [MAX_MESSAGE_LEN] less the stream's header.
    const BATCH_BYTES: usize = ww_hub_api::MAX_BATCH_BYTES;
    const _: () = assert!(BATCH_BYTES + 1024 <= MAX_MESSAGE_LEN);

    pub struct Server {
        pub hub: Arc<Hub>,
        /// The connection's client id: whose claims these are
        pub id: u32,
        /// The client opened `Registry::events`
        pub events_open: bool,
        /// The client opened `ClockSync::stats`
        pub stats_open: bool,
        /// `Adopt::prepare` (CLT-44), until the commit; dropped with the connection
        pub adopting: Option<crate::adopt::Prepared>,
        /// `Captures::watch`es of this connection, by index in `watches[]` (SRV-64); dropped with the connection
        pub watches: Vec<Option<WatchSlot>>,
        pub next_generation: u64,
        /// Where the watches' tasks send their batches
        pub batch_tx: tokio::sync::mpsc::Sender<BatchMsg>,
    }

    /// One watch: its cursor until the stream opens, then the task that reads it.
    pub struct WatchSlot {
        pub generation: u64,
        pub cursor: Option<Box<dyn HostWatch>>,
        pub task: Option<tokio::task::JoinHandle<()>>,
        pub _count: WatchCount,
    }

    impl Drop for WatchSlot {
        fn drop(&mut self) {
            if let Some(task) = &self.task {
                task.abort();
            }
        }
    }

    impl Server {
        async fn info(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<HubInfoOwned> {
            RpcResult::Ready(self.hub.info())
        }

        async fn status(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<HubStatusOwned> {
            let (idle_exit_s, exits_in_s, keep_left_s, connections) = self.hub.timers();
            RpcResult::Ready(HubStatusOwned {
                started_by: self.hub.started_by.clone(),
                idle_exit_s,
                exits_in_s,
                keep_left_s,
                connections,
                captures: self.hub.captures.is_some(),
            })
        }

        async fn handover(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            socket: String,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, HandoverError>>> {
            RpcResult::Ready(self.hub.handover(&socket).await.map_err(Into::into))
        }

        async fn shutdown(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.hub.request_shutdown();
            RpcResult::Ready(())
        }

        async fn keep(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            seconds: u32,
        ) -> RpcResult<()> {
            self.hub.keep(seconds);
            RpcResult::Ready(())
        }

        async fn registry_devices(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<Vec<DeviceEntryOwned>> {
            RpcResult::Ready(crate::devices::entries(&self.hub).await)
        }

        async fn registry_clients(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<Vec<ClientEntryOwned>> {
            let state = self.hub.state.lock().unwrap();
            RpcResult::Ready(
                state
                    .clients
                    .keys()
                    .map(|id| crate::client_entry(&state, *id))
                    .collect(),
            )
        }

        async fn registry_take(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: String,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, TakeErrorOwned>>> {
            RpcResult::Ready(match self.hub.take(&key).await {
                Ok(()) => Ok(()),
                Err(ww_local_socket::Status::HeldElsewhere) => {
                    match wire_weaver_client::holders::holder_of(&key) {
                        Some(holder) => Err(TakeErrorOwned::HeldElsewhere {
                            holder: crate::devices::holder_owned(&holder),
                        }
                        .into()),
                        None => Err(TakeErrorOwned::Failed {
                            why: "held by another program".into(),
                        }
                        .into()),
                    }
                }
                Err(ww_local_socket::Status::Lent) => Err(TakeErrorOwned::Failed {
                    why: format!("lent to {}", self.hub.lent_to(&key).unwrap_or_default()),
                }
                .into()),
                Err(_) => Err(TakeErrorOwned::UnknownDevice.into()),
            })
        }

        async fn claims_claim(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: String,
            path: Vec<UNib32>,
            purpose: String,
            ttl_s: Option<u32>,
        ) -> RpcResult<Result<u32, wire_weaver::Error<'_, ClaimErrorOwned>>> {
            let path = path.iter().map(|n| n.0).collect();
            RpcResult::Ready(
                self.hub
                    .claim(self.id, key, path, purpose, ttl_s)
                    .await
                    .map_err(|e| wire_weaver::Error::Api(*e)),
            )
        }

        async fn claims_release(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: u32,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, ReleaseError>>> {
            RpcResult::Ready(self.hub.release(self.id, id).map_err(Into::into))
        }

        async fn claims_lend(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: String,
            purpose: String,
            ttl_s: Option<u32>,
        ) -> RpcResult<Result<u32, wire_weaver::Error<'_, ClaimErrorOwned>>> {
            RpcResult::Ready(
                self.hub
                    .lend(self.id, key, purpose, ttl_s)
                    .await
                    .map_err(|e| wire_weaver::Error::Api(*e)),
            )
        }

        async fn claims_claims(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: String,
        ) -> RpcResult<Vec<ClaimEntryOwned>> {
            RpcResult::Ready(self.hub.claims_on(&key))
        }

        async fn registry_events(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.events_open = true;
            RpcResult::Ready(())
        }

        async fn close_registry_events(&mut self, _cx: &mut Context<'_, impl EventOut>) {
            self.events_open = false;
        }

        async fn sideband_registry_events(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    }

    /// `ClockSync` (SRV-31).
    impl Server {
        async fn sync_devices(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<Vec<SyncEntryOwned>> {
            RpcResult::Ready(self.hub.steering.lock().unwrap().entries())
        }

        async fn sync_set_enabled(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: Option<String>,
            on: bool,
        ) -> RpcResult<()> {
            self.hub
                .steering
                .lock()
                .unwrap()
                .set_enabled(key.as_deref(), on);
            RpcResult::Ready(())
        }

        async fn set_sync_linger(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: Option<u32>,
        ) -> SetResult<CommonError<'_>> {
            self.hub.steering.lock().unwrap().linger =
                value.map(|s| std::time::Duration::from_secs(s as u64));
            self.hub.recheck_lingers().await;
            SetResult::Set
        }

        async fn get_sync_linger(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<Option<u32>, CommonError<'_>> {
            let linger = self.hub.steering.lock().unwrap().linger;
            GetResult::Value(linger.map(|l| l.as_secs().min(u32::MAX as u64) as u32))
        }

        async fn sync_stats(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            self.stats_open = true;
            RpcResult::Ready(())
        }

        async fn close_sync_stats(&mut self, _cx: &mut Context<'_, impl EventOut>) {
            self.stats_open = false;
        }

        async fn sideband_sync_stats(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }
    }

    /// `Adopt` (CLT-44).
    impl Server {
        async fn adopt_prepare(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            snapshot: Vec<u8>,
        ) -> RpcResult<Result<u64, wire_weaver::Error<'_, AdoptErrorOwned>>> {
            if self.adopting.is_some() {
                return RpcResult::Ready(Err(AdoptErrorOwned::Busy.into()));
            }
            RpcResult::Ready(match crate::adopt::prepare(&self.hub, &snapshot).await {
                Ok(prepared) => {
                    let id = prepared.id;
                    self.adopting = Some(prepared);
                    Ok(id)
                }
                Err(e) => Err(e.into()),
            })
        }

        async fn adopt_commit(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: u64,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, AdoptErrorOwned>>> {
            RpcResult::Ready(match self.adopting.take() {
                Some(prepared) if prepared.id == id => {
                    crate::adopt::commit(&self.hub, self.id, prepared)
                        .await
                        .map_err(Into::into)
                }
                other => {
                    self.adopting = other;
                    Err(AdoptErrorOwned::UnknownId.into())
                }
            })
        }
    }

    /// `Captures` (SRV-64).
    impl Server {
        async fn captures_start(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: MaxLen<String, { ww_hub_api::MAX_KEY_LEN }>,
            options: CaptureOptions,
        ) -> RpcResult<Result<CaptureId, wire_weaver::Error<'_, CaptureErrorOwned>>> {
            RpcResult::Ready(
                self.hub
                    .capture_start(self.id, &key, options)
                    .await
                    .map_err(|e| (*e).into()),
            )
        }

        async fn captures_stop(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, CaptureErrorOwned>>> {
            let stopped = self.hub.registry().and_then(|r| {
                let owner = crate::owner_of(&self.hub.state.lock().unwrap(), self.id);
                r.stop(&owner, id)
            });
            RpcResult::Ready(match stopped {
                Ok(ended) => {
                    if let Some(ended) = ended {
                        self.hub.capture_ended(ended);
                    }
                    Ok(())
                }
                Err(e) => Err((*e).into()),
            })
        }

        async fn captures_save(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
            path: MaxLen<String, { ww_hub_api::MAX_PATH_LEN }>,
        ) -> RpcResult<Result<SaveInfoOwned, wire_weaver::Error<'_, CaptureErrorOwned>>> {
            RpcResult::Ready(self.save(id, &path).await.map_err(|e| (*e).into()))
        }

        async fn captures_discard(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, CaptureErrorOwned>>> {
            RpcResult::Ready(match self.hub.registry().and_then(|r| r.discard(id)) {
                Ok(ended) => {
                    self.hub.capture_ended(ended);
                    Ok(())
                }
                Err(e) => Err((*e).into()),
            })
        }

        async fn captures_set_ram_max(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
            ram_max: RamMax,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, CaptureErrorOwned>>> {
            RpcResult::Ready(
                self.hub
                    .registry()
                    .and_then(|r| r.set_ram_max(id, ram_max))
                    .map_err(|e| (*e).into()),
            )
        }

        async fn captures_captures(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> RpcResult<MaxLen<Vec<CaptureEntryOwned>, { ww_hub_api::MAX_CAPTURES }>> {
            let entries = match &self.hub.captures {
                Some(r) => r.entries(|c| crate::client_entry(&self.hub.state.lock().unwrap(), c)),
                None => vec![],
            };
            RpcResult::Ready(MaxLen::new_unchecked(entries))
        }

        async fn captures_watch(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
            pressure: Pressure,
        ) -> RpcResult<Result<WatchId, wire_weaver::Error<'_, CaptureErrorOwned>>> {
            RpcResult::Ready(self.watch(id, pressure).map_err(|e| (*e).into()))
        }

        fn valid_indices_root_captures_watches(&mut self) -> ValidIndices<'_> {
            ValidIndices::range_u32(0..MAX_WATCHES as u32)
        }

        async fn captures_watches_records(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            index_chain: [ww_client_server::ResourceIndex; 1],
        ) -> RpcResult<()> {
            let w = index_chain[0].get().0;
            if let Some(slot) = self.watches.get_mut(w as usize).and_then(Option::as_mut)
                && let Some(mut cursor) = slot.cursor.take()
            {
                let (tx, generation) = (self.batch_tx.clone(), slot.generation);
                slot.task = Some(tokio::spawn(async move {
                    while let Some(batch) = cursor.next(BATCH_BYTES).await {
                        if tx.send((w, generation, Some(batch))).await.is_err() {
                            return;
                        }
                    }
                    let _ = tx.send((w, generation, None)).await;
                }));
            }
            RpcResult::Ready(())
        }

        async fn close_captures_watches_records(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            index_chain: [ww_client_server::ResourceIndex; 1],
        ) {
            if let Some(slot) = self.watches.get_mut(index_chain[0].get().0 as usize) {
                *slot = None;
            }
        }

        async fn sideband_captures_watches_records(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            _index_chain: [ww_client_server::ResourceIndex; 1],
            _sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            None
        }

        async fn save(
            &mut self,
            id: CaptureId,
            path: &str,
        ) -> Result<SaveInfoOwned, Box<CaptureErrorOwned>> {
            let registry = self.hub.registry()?;
            self.hub.may_save_or_watch(self.id)?;
            let path = SavePath::check(path)?;
            let capture = registry.saving(id, &path)?;
            let to = path.clone();
            // writes what is in RAM: not on the hub's one thread
            let saved = tokio::task::spawn_blocking(move || capture.save(&to))
                .await
                .unwrap_or_else(|e| Err(format!("the save panicked: {e}")));
            match saved {
                Ok(saved) => Ok(SaveInfoOwned {
                    path: path.path().display().to_string(),
                    records: Fixed::new(saved.records),
                    dropped: Fixed::new(saved.dropped),
                }),
                Err(why) => {
                    registry.save_failed(id);
                    Err(CaptureErrorOwned::Host { why }.into())
                }
            }
        }

        fn watch(
            &mut self,
            id: CaptureId,
            pressure: Pressure,
        ) -> Result<WatchId, Box<CaptureErrorOwned>> {
            let registry = self.hub.registry()?;
            self.hub.may_save_or_watch(self.id)?;
            let Some(w) = self.watches.iter().position(Option::is_none) else {
                return Err(CaptureErrorOwned::TooMany.into());
            };
            let (cursor, count) = registry.watch(id, pressure)?;
            self.next_generation += 1;
            self.watches[w] = Some(WatchSlot {
                generation: self.next_generation,
                cursor: Some(cursor),
                task: None,
                _count: count,
            });
            Ok(WatchId(w as u32))
        }
    }

    impl WireWeaverAsyncApiBackend for Server {
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
            ww_hub_api::HUB_FULL_GID
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            ww_hub_api :: Hub for super::Server,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }
}
