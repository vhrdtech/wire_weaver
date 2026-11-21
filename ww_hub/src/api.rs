//! The hub's own API (`ww_hub_api`): a generated `no_alloc = false` server on `wire_weaver_server`, which takes the
//! connections whose preamble asked for the hub (the hub answers every preamble itself) and serves each with a
//! backend of its own until the client leaves or the hub exits (SRV-67).

use crate::locked::Locked;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::StreamExt as _;
use tracing::debug;
use wire_weaver::prelude::*;
use wire_weaver::{DisconnectReason, Peer};
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_client::local_socket::device::{DeviceRx, DeviceTx};
use wire_weaver_server::opened::{OpenedListener, OpenedSender};
use wire_weaver_server::{Connection, Limits, PumpId, Pumps};
use ww_hub_api::{
    AdoptErrorOwned, CaptureErrorOwned, CaptureId, CaptureOptions, CapturePageOwned,
    ClaimEntryOwned, ClaimErrorOwned, ClientEntryOwned, DeviceEntryOwned, HandoverError,
    HubInfoOwned, HubStatusOwned, MAX_WATCHES, Pressure, RamMax, ReleaseError, SaveInfoOwned,
    SyncEntryOwned, TakeErrorOwned, WatchId,
};

use crate::Hub;
use crate::captures::{Act, HostWatch, WatchCount};

/// Longest message on a hub API connection, both ways: device and client lists of a busy bench fit, and a watch's
/// record batch (`ww_hub_api::MAX_BATCH_BYTES`) with its stream header.
pub(crate) const MAX_MESSAGE_LEN: usize = 64 * 1024;
/// Hub API connections served at once: one per program of the user that talks to the hub. Past it a program waits
/// until another leaves.
const CONNECTIONS_MAX: usize = 1024;
/// Hub API connections answered and not yet taken by the server.
const QUEUED_MAX: usize = 16;
/// How long an exiting hub's API connections get to answer what they have before they are disconnected: with the
/// server's linger after the disconnect (200 ms) within the hub's `EXIT_DRAIN`.
const EXIT_GRACE: Duration = Duration::from_millis(250);

/// Where the hub hands the connections of its API.
pub(crate) type Opened = OpenedSender<DeviceTx, DeviceRx>;

/// The server of the hub's API: the sender the hub hands connections to, and the future that serves them until
/// `stop` completes with the reason clients are told, then ends once they are disconnected.
pub(crate) fn server(
    hub: Arc<Hub>,
    stop: impl Future<Output = DisconnectReason> + Send + 'static,
) -> (Opened, impl Future<Output = ()> + Send + 'static) {
    let (opened, listener): (Opened, OpenedListener<_, _>) =
        wire_weaver_server::opened::opened(QUEUED_MAX);
    let serve = async move {
        let link = ww_device::LinkConfig::new(
            ww_hub_api::HUB_FULL_GID,
            ww_hub_api::HUB_WIRE_VERSION,
            server::api_impl::api_hash(),
            server::api_impl::image_facts(),
            ww_client_server::COMPACT_VERSION,
        );
        let limits = Limits {
            connections_max: CONNECTIONS_MAX,
            shutdown_grace: EXIT_GRACE,
            ..Limits::default()
        };
        let make = move |connection: &Connection| server::Server::new(hub.clone(), connection);
        let served = wire_weaver_server::Server::new(TokioClock::new())
            .limits(limits)
            .serve(link, listener, make, stop)
            .await;
        match served {
            Ok(()) => {}
            Err(never) => match never {},
        }
    };
    (opened, serve)
}

/// Runs `work` to its end on a task of its own, also when its request is dropped (its deadline, a cancel, the client
/// left): what it changes in the hub (claims, lends, adoptions, captures, open devices) is never left half done.
async fn to_end<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> T {
    match tokio::spawn(work).await {
        Ok(done) => done,
        Err(e) => match e.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(_) => {
                unreachable!("the runtime ends a task only with the futures that wait for it")
            }
        },
    }
}

/// The pump of `Registry::events`.
const EVENTS: PumpId = PumpId(0);
/// The pump of `ClockSync::stats`.
const SYNC_STATS: PumpId = PumpId(1);

/// The pump of watch `w`'s records.
fn watch_pump(w: u32) -> PumpId {
    PumpId(2 + u64::from(w))
}

mod server {
    use super::*;

    /// What a watch's batch may take of a message: [MAX_MESSAGE_LEN] less the stream's header.
    const BATCH_BYTES: usize = ww_hub_api::MAX_BATCH_BYTES;
    const _: () = assert!(BATCH_BYTES + 1024 <= MAX_MESSAGE_LEN);

    /// The backend of one client of the hub's API.
    pub struct Server {
        hub: Arc<Hub>,
        /// The connection's client id: whose claims these are
        id: u32,
        /// The connection's pumps: the registry's events, sync stats and watches' records as the client opens them
        pumps: Pumps,
        /// `Adopt::prepare` (CLT-44), until the commit; dropped with the connection
        adopting: Option<crate::adopt::Prepared>,
        /// `Captures::watch`es of this connection, by index in `watches[]` (SRV-64); dropped with the connection
        watches: Vec<Option<WatchSlot>>,
    }

    /// One watch: its cursor, counted on its capture, until the stream opens; then its pump has them.
    struct WatchSlot {
        /// In a mutex only to be `Sync`, as the backend is: only `&mut self` handlers reach it
        cursor: std::sync::Mutex<Option<(Box<dyn HostWatch>, WatchCount)>>,
        /// The capture ended: its records are all sent and the slot is free again
        ended: Arc<AtomicBool>,
    }

    impl WatchSlot {
        fn free(slot: &Option<WatchSlot>) -> bool {
            slot.as_ref()
                .is_none_or(|slot| slot.ended.load(Ordering::Acquire))
        }
    }

    impl Server {
        /// The backend of `connection`, whose link is up: the client joins the registry.
        pub fn new(hub: Arc<Hub>, connection: &Connection) -> Self {
            let pid = match connection.peer {
                Peer::Unix { pid, .. } => pid.map(|pid| pid.0),
                _ => None,
            };
            let id = hub.client_joined(pid);
            hub.client_up(id, Some(connection.client.clone()));
            debug!(
                "ww hub: client {id} ({:?}) on the hub's API",
                connection.peer
            );
            Server {
                hub,
                id,
                pumps: connection.pumps.clone(),
                adopting: None,
                watches: (0..MAX_WATCHES).map(|_| None).collect(),
            }
        }
    }

    /// Its claims and its part in captures go with the client, however its connection ends (a panic too).
    impl Drop for Server {
        fn drop(&mut self) {
            debug!("ww hub: client {} left", self.id);
            self.hub.client_left(self.id);
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
            let state = self.hub.state.locked();
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
            let taken = to_end({
                let (hub, key) = (self.hub.clone(), key.clone());
                async move { hub.take(&key).await }
            })
            .await;
            RpcResult::Ready(match taken {
                Ok(()) => Ok(()),
                Err(ww_byte_stream::Status::HeldElsewhere) => {
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
                Err(ww_byte_stream::Status::Lent) => Err(TakeErrorOwned::Failed {
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
            let (hub, id) = (self.hub.clone(), self.id);
            RpcResult::Ready(
                to_end(async move { hub.claim(id, key, path, purpose, ttl_s).await })
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
            let (hub, id) = (self.hub.clone(), self.id);
            RpcResult::Ready(
                to_end(async move { hub.lend(id, key, purpose, ttl_s).await })
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
            let id = self.id;
            let events = self.hub.events.subscribe().filter_map(move |event| {
                std::future::ready(match event {
                    Ok(event) => Some(event),
                    // a slow client misses events, it can read the lists again
                    Err(lagged) => {
                        debug!("ww hub: client {id} missed {} events", lagged.0);
                        None
                    }
                })
            });
            self.pumps.add(EVENTS, events, |event, buf| {
                api_impl::stream_data_ser()
                    .registry()
                    .events(event, buf)
                    .map(<[u8]>::len)
            });
            RpcResult::Ready(())
        }

        async fn close_registry_events(&mut self, _cx: &mut Context<'_, impl EventOut>) {
            self.pumps.remove(EVENTS);
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
            RpcResult::Ready(self.hub.steering.locked().entries())
        }

        async fn sync_set_enabled(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: Option<String>,
            on: bool,
        ) -> RpcResult<()> {
            self.hub.steering.locked().set_enabled(key.as_deref(), on);
            RpcResult::Ready(())
        }

        async fn set_sync_linger(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            value: Option<u32>,
        ) -> SetResult<CommonError<'_>> {
            self.hub.steering.locked().linger =
                value.map(|s| std::time::Duration::from_secs(s as u64));
            self.hub.recheck_lingers().await;
            SetResult::Set
        }

        async fn get_sync_linger(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<Option<u32>, CommonError<'_>> {
            let linger = self.hub.steering.locked().linger;
            GetResult::Value(linger.map(|l| l.as_secs().min(u32::MAX as u64) as u32))
        }

        async fn sync_stats(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
            let id = self.id;
            let stats = self.hub.sync_stats.subscribe().filter_map(move |entry| {
                std::future::ready(match entry {
                    Ok(entry) => Some(entry),
                    // the next update of the device brings it up to date
                    Err(lagged) => {
                        debug!("ww hub: client {id} missed {} sync stats", lagged.0);
                        None
                    }
                })
            });
            self.pumps.add(SYNC_STATS, stats, |entry, buf| {
                api_impl::stream_data_ser()
                    .sync()
                    .stats(entry, buf)
                    .map(<[u8]>::len)
            });
            RpcResult::Ready(())
        }

        async fn close_sync_stats(&mut self, _cx: &mut Context<'_, impl EventOut>) {
            self.pumps.remove(SYNC_STATS);
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
            let hub = self.hub.clone();
            let prepared =
                to_end(async move { crate::adopt::prepare(&hub, &snapshot).await }).await;
            RpcResult::Ready(match prepared {
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
                    let (hub, client) = (self.hub.clone(), self.id);
                    to_end(async move { crate::adopt::commit(&hub, client, prepared).await })
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
            let (hub, id, key) = (self.hub.clone(), self.id, key.as_str().to_owned());
            RpcResult::Ready(
                to_end(async move { hub.capture_start(id, &key, options).await })
                    .await
                    .map_err(|e| (*e).into()),
            )
        }

        async fn captures_stop(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, CaptureErrorOwned>>> {
            let stopped = self
                .hub
                .captures_for(self.id, Act::Stop)
                .and_then(|(r, owner, _)| r.stop(&owner, id));
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
            RpcResult::Ready(
                self.save(id, path.as_str().to_string())
                    .await
                    .map_err(|e| (*e).into()),
            )
        }

        async fn captures_discard(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: CaptureId,
        ) -> RpcResult<Result<(), wire_weaver::Error<'_, CaptureErrorOwned>>> {
            let discarded = self
                .hub
                .captures_for(self.id, Act::Discard)
                .and_then(|(r, owner, _)| r.discard(&owner, id));
            RpcResult::Ready(match discarded {
                Ok(ended) => {
                    // `None`: ends when its running save returns
                    if let Some(ended) = ended {
                        self.hub.capture_ended(ended);
                    }
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
                    .captures_for(self.id, Act::SetRamMax)
                    .and_then(|(r, owner, _)| r.set_ram_max(&owner, id, ram_max))
                    .map_err(|e| (*e).into()),
            )
        }

        async fn captures_captures(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            after: Option<CaptureId>,
        ) -> RpcResult<Result<CapturePageOwned, wire_weaver::Error<'_, CaptureErrorOwned>>>
        {
            RpcResult::Ready(
                self.hub
                    .captures_for(self.id, Act::List)
                    .map(|(r, _, _)| r.page(after, |owner| self.hub.live_connection(owner)))
                    .map_err(|e| (*e).into()),
            )
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
                && let Some(cursor) = slot
                    .cursor
                    .get_mut()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
            {
                // one batch in flight: a client slower than its watches makes the pump wait, which makes the host's
                // cursors fall behind, never the device (SRV-64)
                let ended = slot.ended.clone();
                // the cursor and its count go when the capture ends or the stream closes
                let batches = futures_util::stream::unfold(cursor, move |(mut cursor, count)| {
                    let ended = ended.clone();
                    async move {
                        let batch = cursor.next(BATCH_BYTES).await;
                        if batch.is_none() {
                            ended.store(true, Ordering::Release);
                        }
                        batch.map(|batch| (batch, (cursor, count)))
                    }
                });
                self.pumps.add(watch_pump(w), batches, move |batch, buf| {
                    api_impl::stream_data_ser()
                        .captures()
                        .watches(w)
                        .records(batch, buf)
                        .map(<[u8]>::len)
                });
            }
            RpcResult::Ready(())
        }

        async fn close_captures_watches_records(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            index_chain: [ww_client_server::ResourceIndex; 1],
        ) {
            let w = index_chain[0].get().0;
            if let Some(slot) = self.watches.get_mut(w as usize) {
                self.pumps.remove(watch_pump(w));
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
            path: String,
        ) -> Result<SaveInfoOwned, Box<CaptureErrorOwned>> {
            let (registry, _, _) = self.hub.captures_for(self.id, Act::Save)?;
            let running = registry.save_running(&path);
            let hub = self.hub.clone();
            // the path's checks and the save touch the file system, which may hang (a network mount): never on the
            // hub's one thread. The capture's end waits for the save, also when this connection goes meanwhile.
            tokio::task::spawn_blocking(move || {
                let _running = running;
                let (result, ended) = hub.registry()?.save(id, &path);
                if let Some(ended) = ended {
                    hub.capture_ended(ended);
                }
                result
            })
            .await
            .unwrap_or_else(|e| {
                Err(CaptureErrorOwned::Host {
                    why: format!("the save panicked: {e}"),
                }
                .into())
            })
        }

        fn watch(
            &mut self,
            id: CaptureId,
            pressure: Pressure,
        ) -> Result<WatchId, Box<CaptureErrorOwned>> {
            let (registry, _, _) = self.hub.captures_for(self.id, Act::Watch)?;
            let Some(w) = self.watches.iter().position(WatchSlot::free) else {
                return Err(CaptureErrorOwned::TooMany.into());
            };
            let (cursor, count) = registry.watch(id, pressure)?;
            // the pump of a capture that ended before is done: nothing of it is sent on the new watch
            self.pumps.remove(watch_pump(w as u32));
            self.watches[w] = Some(WatchSlot {
                cursor: std::sync::Mutex::new(Some((cursor, count))),
                ended: Arc::new(AtomicBool::new(false)),
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

    /// Every request runs alone, in the order it came: the hub's handlers change state shared by all clients.
    impl WireWeaverSharedApiBackend for Server {
        async fn process_exclusive_bytes<'a>(
            &mut self,
            origin: &CallContext,
            out: &mut EventWriter<'_, impl MessageSink>,
            medium: (),
            data: &[u8],
            scratch: &'a mut [u8],
        ) -> Result<&'a [u8], ShrinkWrapError> {
            self.process_request_from(origin, data, scratch, out, medium)
                .await
        }

        fn is_shared(data: &[u8]) -> bool {
            Server::is_shared(data)
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

    /// Streams are pumps: nothing to send between requests.
    impl wire_weaver_server::Backend for Server {}

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            ww_hub_api :: Hub for super::Server,
            server = true, no_alloc = false, use_async = true,
            method_model = "_=immediate",
            property_model = "_=get_set",
        );
    }
}
