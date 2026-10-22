//! The hub's own API (`ww_hub_api`) on one connection: a generated `no_alloc = false` server on `ww_device::Server`,
//! over the connection whose preamble the hub already answered.

use std::sync::Arc;

use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;
use wire_weaver::prelude::*;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_client::local_socket::device::connection_medium;
use ww_device::LinkEvent;
use ww_hub_api::{
    AdoptErrorOwned, ClaimEntryOwned, ClaimErrorOwned, ClientEntryOwned, DeviceEntryOwned,
    HandoverError, HubInfoOwned, HubStatusOwned, ReleaseError, SyncEntryOwned, TakeErrorOwned,
};

use crate::Hub;

/// Longest message on a hub API connection, both ways: device and client lists of a busy bench fit.
const MAX_MESSAGE_LEN: usize = 16 * 1024;

/// Serves the hub API to one client until it leaves.
pub(crate) async fn serve_connection(hub: Arc<Hub>, id: u32, rx: RecvHalf, tx: SendHalf) {
    let (tx, rx, mut ended) = connection_medium(rx, tx, MAX_MESSAGE_LEN);
    let mut scratch = vec![0u8; MAX_MESSAGE_LEN];
    let mut event_scratch = vec![0u8; MAX_MESSAGE_LEN];
    let config = ww_device::LinkConfig::new(
        ww_hub_api::HUB_FULL_GID,
        server::api_impl::api_hash(),
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
    let mut backend = server::Server {
        hub: hub.clone(),
        id,
        events_open: false,
        stats_open: false,
        adopting: None,
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
    }

    impl Server {
        async fn info(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<HubInfoOwned> {
            RpcResult::Ready(HubInfoOwned {
                hub_version: self.hub.version.clone(),
                // ww_hub is versioned with the framework crates (workspace version)
                ww_version: env!("CARGO_PKG_VERSION").to_string(),
                pid: std::process::id(),
                exe: std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                started_ms: self.hub.started_ms,
                socket: self.hub.socket.display().to_string(),
            })
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
            })
        }

        async fn handover(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            socket: String,
        ) -> RpcResult<Result<(), HandoverError>> {
            RpcResult::Ready(self.hub.handover(&socket).await)
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
        ) -> RpcResult<Result<(), TakeErrorOwned>> {
            RpcResult::Ready(match self.hub.take(&key).await {
                Ok(()) => Ok(()),
                Err(ww_local_socket::Status::HeldElsewhere) => {
                    match wire_weaver_client::holders::holder_of(&key) {
                        Some(holder) => Err(TakeErrorOwned::HeldElsewhere {
                            holder: crate::devices::holder_owned(&holder),
                        }),
                        None => Err(TakeErrorOwned::Failed {
                            why: "held by another program".into(),
                        }),
                    }
                }
                Err(ww_local_socket::Status::Lent) => Err(TakeErrorOwned::Failed {
                    why: format!("lent to {}", self.hub.lent_to(&key).unwrap_or_default()),
                }),
                Err(_) => Err(TakeErrorOwned::UnknownDevice),
            })
        }

        async fn claims_claim(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: String,
            path: Vec<UNib32>,
            purpose: String,
            ttl_s: Option<u32>,
        ) -> RpcResult<Result<u32, ClaimErrorOwned>> {
            let path = path.iter().map(|n| n.0).collect();
            RpcResult::Ready(
                self.hub
                    .claim(self.id, key, path, purpose, ttl_s)
                    .await
                    .map_err(|e| *e),
            )
        }

        async fn claims_release(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: u32,
        ) -> RpcResult<Result<(), ReleaseError>> {
            RpcResult::Ready(self.hub.release(self.id, id))
        }

        async fn claims_lend(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            key: String,
            purpose: String,
            ttl_s: Option<u32>,
        ) -> RpcResult<Result<u32, ClaimErrorOwned>> {
            RpcResult::Ready(
                self.hub
                    .lend(self.id, key, purpose, ttl_s)
                    .await
                    .map_err(|e| *e),
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
        ) -> SetResult<()> {
            self.hub.steering.lock().unwrap().linger =
                value.map(|s| std::time::Duration::from_secs(s as u64));
            self.hub.recheck_lingers().await;
            SetResult::Set
        }

        async fn get_sync_linger(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
        ) -> GetResult<Option<u32>, ()> {
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
        ) -> RpcResult<Result<u64, AdoptErrorOwned>> {
            if self.adopting.is_some() {
                return RpcResult::Ready(Err(AdoptErrorOwned::Busy));
            }
            RpcResult::Ready(match crate::adopt::prepare(&self.hub, &snapshot).await {
                Ok(prepared) => {
                    let id = prepared.id;
                    self.adopting = Some(prepared);
                    Ok(id)
                }
                Err(e) => Err(e),
            })
        }

        async fn adopt_commit(
            &mut self,
            _cx: &mut Context<'_, impl EventOut>,
            id: u64,
        ) -> RpcResult<Result<(), AdoptErrorOwned>> {
            RpcResult::Ready(match self.adopting.take() {
                Some(prepared) if prepared.id == id => {
                    crate::adopt::commit(&self.hub, self.id, prepared).await
                }
                other => {
                    self.adopting = other;
                    Err(AdoptErrorOwned::UnknownId)
                }
            })
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
