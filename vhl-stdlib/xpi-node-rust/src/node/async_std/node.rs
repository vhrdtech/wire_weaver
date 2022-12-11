use crate::node::addressing::RemoteNodeAddr;
use crate::node::async_std::NodeError;
use crate::node::filter::EventFilter;
use crate::remote::remote_descriptor::RemoteDescriptor;
use crate::remote::tcp::{tcp_event_loop, tcp_server_acceptor};
use core::time::Duration;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures::{SinkExt, Stream, StreamExt};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, instrument, trace, warn};
use xpi::node_set::XpiGenericNodeSet;
use xpi::owned::node_id::NodeId;
use xpi::owned::Event;
use xpi::owned::Priority;
use xpi::owned::RequestId;
use crate::node::async_std::internal_event::InternalEvent;

#[derive(Debug)]
pub struct VhNode {
    id: NodeId,
    tx_to_event_loop: Sender<Event>,
    tx_internal: Sender<InternalEvent>,
}

impl VhNode {
    /// Create a node with a sole purpose of sending requests to another nodes.
    ///
    /// Created node will contain xPI implementations of: semver, client and will answer to respective
    /// requests. Heartbeats will also be broadcasted.
    pub async fn new_client(
        id: NodeId, /* xPI client, generated or dynamically loaded */
    ) -> VhNode {
        let (tx_to_event_loop, rx_router) = mpsc::channel(64); // TODO: config
        let (tx_internal, rx_internal) = mpsc::channel(16);
        tokio::spawn(async move {
            Self::process_events(id, rx_router, rx_internal).await;
        });
        VhNode {
            id,
            tx_to_event_loop,
            tx_internal,
            // nodes,
        }
    }

    pub async fn new_server(
        id: NodeId, /* xPI server, generated or dynamically loaded */
    ) -> VhNode {
        let (tx_to_event_loop, rx_router) = mpsc::channel(64); // TODO: config
        let (tx_internal, rx_internal) = mpsc::channel(16);
        tokio::spawn(async move {
            Self::process_events(id, rx_router, rx_internal).await;
        });
        VhNode {
            id,
            tx_to_event_loop,
            tx_internal,
            // nodes,
        }
    }

    pub fn node_id(&self) -> NodeId {
        self.id
    }

    // pub async fn new_server
    // pub async fn new_router
    // pub async fn new_tracer

    /// Process all the xPI events that may be received from other nodes and send it's own.
    /// Route local traffic between nodes or through one of the transport channels to the
    /// outside world if routing is enabled.
    ///
    /// # Arguments
    /// rx_from_nodes: any other software node can send an event here.
    /// rx_internal: when new node is added, message is sent to this channel and it's handle is
    ///     Option::take()'n into local nodes hashmap.
    #[instrument(skip(rx_from_instances, rx_internal))]
    async fn process_events(
        self_node_id: NodeId,
        mut rx_from_instances: Receiver<Event>,
        mut rx_internal: Receiver<InternalEvent>,
    ) {
        info!("Entering event loop");
        // send out heartbeats
        // answer introspects
        // process read/write/subscribe
        // process method calls
        // process timeouts
        // rate shaper?

        // tx handles to another node instances running on the same executor
        let mut nodes: HashMap<NodeId, Sender<Event>> = HashMap::new();

        // tx handles to another nodes running on remote machines or in another processes
        let mut remote_nodes: Vec<RemoteDescriptor> = Vec::new();

        // tx handles to Self for filter_one and filter_many
        let mut filters: Vec<(EventFilter, Sender<Event>)> = Vec::new();

        let heartbeat = tick_stream(Duration::from_secs(1)).fuse();
        // let mut heartbeat = tokio::time::interval(Duration::from_millis(1000));
        let mut uptime: u32 = 0;
        let mut heartbeat_request_id: u32 = 0;

        futures::pin_mut!(heartbeat);
        loop {
            futures::select! {
                ev = rx_from_instances.select_next_some() => {
                    Self::process_events_from_instances(
                        self_node_id,
                        &ev,
                        &mut filters,
                        &mut remote_nodes
                    ).await;
                }
                ev_int = rx_internal.select_next_some() => {
                    match ev_int {
                        InternalEvent::ConnectInstance(id, tx_handle) => {
                            nodes.insert(id, tx_handle);
                            info!("{}: connected to {} (executor local)", self_node_id.0, id.0);
                        }
                        InternalEvent::DisconnectInstance(id) => {
                            nodes.remove(&id);
                            info!("{}: disconnected from {}", self_node_id.0, id.0);
                        }
                        InternalEvent::Filter(filter, tx_handle) => {
                            let idx = filters.len();
                            info!("filter {:?} registered with idx {idx}", filter);
                            filters.push((filter, tx_handle));
                        }
                        InternalEvent::ConnectRemoteTcp(remote_descriptor) => {
                            info!("remote attachment {} registered", remote_descriptor);
                            remote_nodes.push(remote_descriptor);
                        }
                        InternalEvent::DropRemoteTcp(remote_addr) => {
                            info!("remote attachment {remote_addr} is being dropped");
                            remote_nodes.retain(|rd| rd.addr != RemoteNodeAddr::Tcp(remote_addr));
                        }
                    }
                }
                // tcp_rx_res = tcp_streams_rx => {
                //     println!("tcp rx: {:?}", tcp_rx_res);
                // }
                // _ = heartbeat.tick() => {
                _ = heartbeat.next() => {
                    trace!("{}: local heartbeat", self_node_id.0);
                    let heartbeat_ev = Event::new_heartbeat(self_node_id, RequestId(heartbeat_request_id), Priority::Lossy(0), uptime);
                    for rd in &mut remote_nodes {
                        if rd.to_event_loop.send(heartbeat_ev.clone()).await.is_err() {
                            error!("Failed to forward heartbeat to remote attachment event loop of: {:?}", rd.addr);
                        }
                    }
                    uptime += 1;
                    heartbeat_request_id += 1;

                    for (node_id, sender) in &nodes {
                        if sender.is_closed() {
                            warn!("Node instance with node id {node_id} is down");
                        }
                    }
                    for remote_node in &remote_nodes {
                        if remote_node.to_event_loop.is_closed() {
                            warn!("Remote node attachment to {:?} is down", remote_node.reachable);
                        }
                    }
                }
                complete => {
                    warn!("{}: unexpected complete", self_node_id.0);
                    break;
                }
            }
        }
    }

    async fn process_events_from_instances(
        self_node_id: NodeId,
        ev: &Event,
        filters: &mut Vec<(EventFilter, Sender<Event>)>,
        remote_nodes: &mut Vec<RemoteDescriptor>,
    ) {
        trace!("rx_from_instances: {}", ev);
        let mut filters_to_drop = vec![];
        let mut forwards_count = 0;
        for (idx, (filter, tx_handle)) in filters.iter_mut().enumerate() {
            if filter.matches(&ev) {
                let r = tx_handle.send(ev.clone()).await; // TODO: count
                if r.is_ok() {
                    forwards_count += 1;
                }
                if r.is_err() || filter.is_single_shot() {
                    filters_to_drop.push(idx);
                }
            }
        }
        trace!("forwarded to {forwards_count} instances");
        for f in filters_to_drop {
            trace!("dropping filter {f}");
            filters.remove(f);
        }

        match ev.destination {
            XpiGenericNodeSet::Unicast(id) => {
                if self_node_id != id {
                    // && routing_enabled
                    for rd in remote_nodes {
                        if rd.reachable.contains(&id) {
                            if rd.to_event_loop.send(ev.clone()).await.is_ok() {
                                trace!("Forwarded to attachment event loop: {:?}", rd.addr);
                            } else {
                                error!("Failed to forward event to remote attachment event loop of: {:?}", rd.addr);
                            }
                        }
                    }
                }
            }
            XpiGenericNodeSet::UnicastTraits { .. } => unimplemented!(),
            XpiGenericNodeSet::Multicast { .. } => unimplemented!(),
            XpiGenericNodeSet::Broadcast { .. } => {
                // if self_node_id != original_source {
                //     for rd in remote_nodes {
                //         if rd.to_event_loop.send(ev.clone()).await.is_ok() {
                //             trace!("Forwarded broadcast to: {:?}", rd.addr);
                //         } else {
                //             error!(
                //                 "Failed to forward event to remote attachment event loop of: {:?}",
                //                 rd.addr
                //             );
                //         }
                //     }
                // }
            }
        }
        // if routing is enabled
    }

    pub fn new_tx_handle(&self) -> Sender<Event> {
        self.tx_to_event_loop.clone()
    }

    async fn connect_instance(&mut self, other: &mut VhNode) -> Result<(), NodeError> {
        self.tx_internal
            .send(InternalEvent::ConnectInstance(
                other.id,
                other.tx_to_event_loop.clone(),
            ))
            .await?;

        Ok(())
    }

    async fn disconnect_instance(&mut self, other_node_id: NodeId) -> Result<(), NodeError> {
        self.tx_internal.send(InternalEvent::DisconnectInstance(other_node_id)).await?;
        Ok(())
    }

    pub async fn connect_instances(node_a: &mut Self, node_b: &mut Self) -> Result<(), NodeError> {
        node_a.connect_instance(node_b).await?;
        match node_b.connect_instance(node_a).await {
            Ok(_) => Ok(()),
            Err(_) => {
                node_a.disconnect_instance(node_b.id).await?;
                Ok(())
            }
        }
    }

    #[instrument(skip(self), fields(node_id = self.id.0))]
    pub async fn connect_remote(&mut self, addr: RemoteNodeAddr, remote_reachable: Vec<NodeId>) -> Result<(), NodeError> {
        match addr {
            RemoteNodeAddr::Tcp(ip_addr) => {
                info!("tcp: Connecting to remote");
                let tcp_stream = TcpStream::connect(ip_addr).await?;
                info!("Connected");
                let (tx, rx) = mpsc::channel(64);
                let id = self.id.clone();
                let to_event_loop = self.tx_to_event_loop.clone();
                let to_event_loop_internal = self.tx_internal.clone();
                tokio::spawn(async move {
                    tcp_event_loop(id, ip_addr, tcp_stream, to_event_loop.clone(), to_event_loop_internal, rx).await
                });
                let remote_descriptor = RemoteDescriptor {
                    reachable: remote_reachable,
                    addr,
                    to_event_loop: tx,
                };
                self.tx_internal
                    .send(InternalEvent::ConnectRemoteTcp(remote_descriptor))
                    .await?;
                Ok(())
            }
        }
    }

    #[instrument(skip(self), fields(node_id = self.id.0))]
    pub async fn listen(&mut self, addr: RemoteNodeAddr) -> Result<(), NodeError> {
        match addr {
            RemoteNodeAddr::Tcp(ip_addr) => {
                let listener = TcpListener::bind(ip_addr).await?;
                info!("tcp: Listening on: {ip_addr}");

                let id = self.id.clone();
                let tx_to_event_loop = self.tx_to_event_loop.clone();
                let tx_internal = self.tx_internal.clone();
                tokio::spawn(async move {
                    tcp_server_acceptor(id, listener, tx_to_event_loop, tx_internal).await
                });

                Ok(())
            }
        }
    }

    /// Send event to the event loop and return immediately. Event will be sent to another node or nodes
    /// directly or through one of the interfaces available depending on the destination.
    pub async fn submit_one(&mut self, ev: Event) -> Result<(), NodeError> {
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
    pub async fn filter_one(&mut self, filter: EventFilter) -> Result<Event, NodeError> {
        let (tx, mut rx) = mpsc::channel(1);
        self.tx_internal
            .send(InternalEvent::Filter(filter.single_shot(true), tx))
            .await?;
        let ev = rx.next().await.ok_or(NodeError::FilterOneFail)?;
        Ok(ev)
    }

    /// Get a stream source with only the desired events in it.
    /// For subscribing to property updates and streams.
    pub async fn filter_many(&mut self, filter: EventFilter) -> Result<Receiver<Event>, NodeError> {
        let (tx, rx) = mpsc::channel(1);
        self.tx_internal
            .send(InternalEvent::Filter(filter.single_shot(false), tx))
            .await?;
        Ok(rx)
    }
}

fn tick_stream(period: Duration) -> impl Stream<Item=()> {
    futures::stream::unfold(period, move |p| async move {
        tokio::time::sleep(period).await;
        Some(((), p))
    })
}


// async fn outgoing_process(mut tcp_tx: OwnedWriteHalf, mut mpsc_rx: Receiver<VhLinkEvent>) {
//     while let Some(ev) = mpsc_rx.next().await {
//         println!("outgoing_process: got event: {:?}", ev);
//         let r = tcp_tx.write_all(&[0x3, 0xa1, 0x16, 0x60, 0x10, 0x52, 0x55, 0x19,  0x0,  0xa,  0x0, 0x14,  0x0,  0x5,  0x0,  0x7,  0x0, 0x1b]).await;
//     }
// }
