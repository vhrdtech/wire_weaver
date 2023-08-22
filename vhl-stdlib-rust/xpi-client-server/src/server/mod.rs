pub mod error;
mod internal_event;

pub use internal_event::DispatcherHandle;
mod remote_descriptor;
pub mod ws;

use crate::filter::EventFilter;
use core::time::Duration;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures::{SinkExt, StreamExt};
use internal_event::InternalEventToEventLoop;
use remote_descriptor::RemoteDescriptor;
use tokio::net::TcpListener;
use tracing::{error, info, instrument, trace, warn};
use xpi::client_server_owned::{AddressableEvent, Protocol};

use error::NodeError;
use internal_event::InternalEvent;

pub mod prelude {
    pub use super::error::NodeError;
    pub use super::Server;
    pub use crate::filter::EventFilter;
    pub use xpi::client_server_owned::prelude::*;
    pub use xpi::client_server_owned::AddressableEvent;
    pub use xpi::error::XpiError;
}

#[derive(Debug)]
pub struct Server {
    tx_to_event_loop: Sender<AddressableEvent>,
    tx_internal: Sender<InternalEvent>,
    tx_control: Sender<ServerControlRequest>,
}

#[derive(Clone)]
pub struct SubGroupHandle {
    pub filter: EventFilter,
    pub tx: Sender<AddressableEvent>,
}

#[derive(Debug)]
pub enum ServerControlRequest {
    RegisterDispatcher(DispatcherHandle),
}

impl Server {
    pub async fn new() -> Server {
        let (tx_to_event_loop, rx_router) = mpsc::channel(64); // TODO: config
        let (tx_internal, rx_internal) = mpsc::channel(16);
        let (tx_control, rx_control) = mpsc::channel(16);
        tokio::spawn(async move {
            Self::event_loop(rx_router, rx_internal, rx_control).await;
        });
        Server {
            tx_to_event_loop,
            tx_internal,
            tx_control,
        }
    }

    /// Process all the xPI events that may be received from other nodes and send it's own.
    /// Route local traffic between nodes or through one of the transport channels to the
    /// outside world if routing is enabled.
    ///
    /// # Arguments
    /// rx_from_nodes: any other software node can send an event here.
    /// rx_internal: when new node is added, message is sent to this channel and it's handle is
    ///     Option::take()'n into local nodes hashmap.
    #[instrument(skip(rx_from_instances, rx_internal, rx_control))]
    async fn event_loop(
        mut rx_from_instances: Receiver<AddressableEvent>,
        mut rx_internal: Receiver<InternalEvent>,
        mut rx_control: Receiver<ServerControlRequest>,
    ) {
        info!("Entering event loop");

        // tx handles to each client connection's event loop
        let mut clients: Vec<RemoteDescriptor> = Vec::new();
        // tx handles to Self for filter_one and filter_many
        let mut filters: Vec<(EventFilter, Sender<AddressableEvent>)> = Vec::new();

        let heartbeat = crate::util::tick_stream(Duration::from_secs(1)).fuse();
        // let mut heartbeat = tokio::time::interval(Duration::from_millis(1000));
        let _uptime: u32 = 0;
        let _heartbeat_request_id: u32 = 0;

        futures::pin_mut!(heartbeat);
        loop {
            futures::select! {
                ev = rx_from_instances.select_next_some() => {
                    Self::process_events(
                        &ev,
                        &mut filters,
                        &mut clients
                    ).await;
                }
                ev_int = rx_internal.select_next_some() => {
                    Self::process_internal_events(
                        ev_int,
                        // &mut nodes,
                        &mut filters,
                        &mut clients
                    ).await;
                }
                ev_control = rx_control.select_next_some() => {
                    Self::process_control_request(ev_control, &mut clients).await;
                }
                _ = heartbeat.next() => {
                    // trace!("{}: local heartbeat", id.0);
                    // let heartbeat_ev = Event::new_heartbeat(id, RequestId(heartbeat_request_id), Priority::Lossy(0), uptime);
                    // for rd in &mut remote_nodes {
                    //     if rd.to_event_loop.send(heartbeat_ev.clone()).await.is_err() {
                    //         error!("Failed to forward heartbeat to remote attachment event loop of: {:?}", rd.addr);
                    //     }
                    // }
                    // uptime += 1;
                    // heartbeat_request_id += 1;

                    // Drop disconnected clients - not needed since InternalEvent::DropRemote does this
                    // clients.retain(|attch| {
                    //     if attch.to_event_loop.is_closed() {
                    //         warn!("Remote node attachment to {} is down, dropping", attch.protocol);
                    //         return false;
                    //     }
                    //     true
                    // });

                    Self::drop_timed_out_filters(&mut filters);
                }
                complete => {
                    warn!("unexpected complete");
                    break;
                }
            }
        }
    }

    async fn process_events(
        ev: &AddressableEvent,
        filters: &mut Vec<(EventFilter, Sender<AddressableEvent>)>,
        remote_nodes: &mut Vec<RemoteDescriptor>,
    ) {
        let mut filters_to_drop = vec![];
        let mut forwards_count = 0;
        for (idx, (filter, tx_handle)) in filters.iter_mut().enumerate() {
            if !ev.is_inbound {
                // event is supposed to be sent to on of the clients, see below
                continue;
            }
            if filter.matches(&ev.event) {
                let r = tx_handle.send(ev.clone()).await;
                if r.is_ok() {
                    forwards_count += 1;
                }
                if r.is_err() || filter.is_single_shot() {
                    filters_to_drop.push(idx);
                }
            }
        }
        for f in filters_to_drop {
            trace!("dropping filter {f}");
            filters.remove(f);
        }

        let mut attachments_addrs = vec![];

        for rd in remote_nodes {
            if !ev.is_inbound && rd.protocol == ev.protocol {
                if rd.to_event_loop.send(ev.clone()).await.is_ok() {
                    // trace!("Forwarded to attachment event loop: {:?}", rd.addr);
                    attachments_addrs.push(rd.protocol);
                } else {
                    error!(
                        "Failed to forward event to remote attachment event loop of: {}",
                        rd.protocol
                    );
                }
            }
        }
        trace!(
            "rx from instances: {ev} -> {forwards_count} instances and -> {attachments_addrs:?}"
        );
    }

    async fn process_internal_events(
        ev: InternalEvent,
        // nodes: &mut HashMap<NodeId, Sender<AddressableEvent>>,
        filters: &mut Vec<(EventFilter, Sender<AddressableEvent>)>,
        remote_nodes: &mut Vec<RemoteDescriptor>,
    ) {
        match ev {
            InternalEvent::Filter(filter, tx_handle) => {
                let idx = filters.len();
                info!("filter {:?} registered with idx {idx}", filter);
                filters.push((filter, tx_handle));
            }
            InternalEvent::ConnectRemote(remote_descriptor) => {
                info!("remote attachment {:?} registered", remote_descriptor);
                remote_nodes.push(remote_descriptor);
            }
            InternalEvent::DropRemote(remote_addr) => {
                info!("remote attachment {remote_addr} is being dropped");
                let mut idx_to_drop = None;
                for (idx, rd) in remote_nodes.iter().enumerate() {
                    if rd.protocol == remote_addr {
                        idx_to_drop = Some(idx);
                        // let r = rd
                        //     .to_event_loop_internal
                        //     .send(InternalEventToEventLoop::DropAllRelatedTo(remote_addr))
                        //     .await;
                        // trace!("sending drop related to {}: {r:?}", rd.protocol);
                        break;
                    }
                }
                if let Some(idx) = idx_to_drop {
                    remote_nodes.remove(idx);
                }
            }
        }
    }

    pub fn control_tx_handle(&self) -> Sender<ServerControlRequest> {
        self.tx_control.clone()
    }

    async fn process_control_request(
        ev: ServerControlRequest,
        clients: &mut Vec<RemoteDescriptor>,
    ) {
        match ev {
            ServerControlRequest::RegisterDispatcher(handle) => {
                let remote = clients.iter_mut().find(|d| d.protocol == handle.protocol);
                let Some(remote) = remote else {
                    warn!("Control request to {}: client event loop not found", handle.protocol);
                    return;
                };
                let r = remote
                    .to_event_loop_internal
                    .send(InternalEventToEventLoop::RegisterDispatcher(handle))
                    .await;
                if r.is_err() {
                    warn!("Control request to {}: send error", remote.protocol);
                }
            }
        }
    }

    fn drop_timed_out_filters(filters: &mut Vec<(EventFilter, Sender<AddressableEvent>)>) {
        let filters_len_pre = filters.len();
        filters.retain(|(filter, _)| !filter.is_timed_out());
        let diff = filters_len_pre - filters.len();
        if diff > 0 {
            warn!("Dropped {diff} filters due to timeout");
        }
    }

    pub fn new_tx_handle(&self) -> Sender<AddressableEvent> {
        self.tx_to_event_loop.clone()
    }

    #[instrument(skip(self, subgroup_handlers))]
    pub async fn listen(
        &mut self,
        protocol: Protocol,
        subgroup_handlers: Vec<SubGroupHandle>,
    ) -> Result<(), NodeError> {
        let tx_to_event_loop = self.tx_to_event_loop.clone();
        let tx_internal = self.tx_internal.clone();
        match protocol {
            Protocol::Tcp { .. } => {
                // let listener = TcpListener::bind(ip_addr).await?;
                // info!("tcp: Listening on: {ip_addr}");

                // tokio::spawn(async move {
                //     tcp_server_acceptor(id, listener, tx_to_event_loop, tx_internal).await
                // });

                // Ok(())
                unimplemented!()
            }
            Protocol::Ws { ip_addr, port } => {
                let listener = TcpListener::bind((ip_addr, port)).await?;
                info!("ws: Listening on: {ip_addr}:{port}");

                tokio::spawn(async move {
                    ws::ws_server_acceptor(
                        listener,
                        subgroup_handlers,
                        tx_to_event_loop,
                        tx_internal,
                    )
                    .await
                });

                Ok(())
            }
        }
    }

    /// Send event to the event loop and return immediately. Event will be sent to another node or nodes
    /// directly or through one of the interfaces available depending on the destination.
    pub async fn submit_one(&mut self, ev: AddressableEvent) -> Result<(), NodeError> {
        self.tx_to_event_loop.send(ev).await?;
        Ok(())
    }

    /// Get a stream sink to which events can be streamed asynchronously.
    /// For streaming property updates or streams out of this node.
    /// Ensure that source is actually this node? or pre-configure source, dest and prio, expecting only kind?
    pub async fn submit_many(&mut self) -> u32 {
        todo!()
    }

    /// Wait for a particular event or timeout.
    /// For waiting for a reply to previously sent request.
    ///
    /// Internally a temporary channel is created, tx end of which is transferred to the event loop.
    /// Then we await or timeout on rx end of that channel for a response.
    /// Afterwards the channel is dropped.
    pub async fn filter_one(&mut self, filter: EventFilter) -> Result<AddressableEvent, NodeError> {
        let (tx, mut rx) = mpsc::channel(1);
        let timeout = filter.timeout();
        self.tx_internal
            .send(InternalEvent::Filter(filter.single_shot(true), tx))
            .await?;
        let ev = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, rx.next())
                .await
                .map_err(|_| NodeError::Timeout)?
                .ok_or(NodeError::FilterOneFail)?,
            None => rx.next().await.ok_or(NodeError::FilterOneFail)?,
        };
        Ok(ev)
    }

    /// Get a stream source with only the desired events in it.
    /// For subscribing to property updates and streams.
    pub async fn filter_many(
        &mut self,
        filter: EventFilter,
    ) -> Result<Receiver<AddressableEvent>, NodeError> {
        let (tx, rx) = mpsc::channel(1);
        self.tx_internal
            .send(InternalEvent::Filter(filter.single_shot(false), tx))
            .await?;
        Ok(rx)
    }
}
