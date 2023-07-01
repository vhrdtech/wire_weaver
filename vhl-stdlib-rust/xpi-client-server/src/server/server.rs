use super::remote_descriptor::RemoteDescriptor;
use crate::filter::EventFilter;
use core::time::Duration;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures::{SinkExt, Stream, StreamExt};
use tokio::net::TcpListener;
use tracing::{error, info, instrument, trace, warn};
use xpi::client_server_owned::{AddressableEvent, Protocol};

use super::internal_event::InternalEvent;
use super::NodeError;

#[derive(Debug)]
pub struct Server {
    tx_to_event_loop: Sender<AddressableEvent>,
    tx_internal: Sender<InternalEvent>,
}

impl Server {
    // Create a node with a sole purpose of sending requests to another nodes.
    //
    // Created node will contain xPI implementations of: semver, client and will answer to respective
    // requests. Heartbeats will also be broadcasted.
    // pub async fn new_client(
    //     id: NodeId, /* xPI client, generated or dynamically loaded */
    // ) -> Server {
    //     let (tx_to_event_loop, rx_router) = mpsc::channel(64); // TODO: config
    //     let (tx_internal, rx_internal) = mpsc::channel(16);
    //     tokio::spawn(async move {
    //         Self::process_events(id, rx_router, rx_internal).await;
    //     });
    //     Server {
    //         id,
    //         tx_to_event_loop,
    //         tx_internal,
    //         // nodes,
    //     }
    // }

    pub async fn new() -> Server {
        let (tx_to_event_loop, rx_router) = mpsc::channel(64); // TODO: config
        let (tx_internal, rx_internal) = mpsc::channel(16);
        tokio::spawn(async move {
            Self::process_events(rx_router, rx_internal).await;
        });
        Server {
            tx_to_event_loop,
            tx_internal,
            // nodes,
        }
    }

    // pub fn node_id(&self) -> NodeId {
    //     self.id
    // }

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
        mut rx_from_instances: Receiver<AddressableEvent>,
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
        // let mut nodes: HashMap<NodeId, Sender<AddressableEvent>> = HashMap::new();

        // tx handles to another nodes running on remote machines or in another processes
        let mut remote_nodes: Vec<RemoteDescriptor> = Vec::new();

        // tx handles to Self for filter_one and filter_many
        let mut filters: Vec<(EventFilter, Sender<AddressableEvent>)> = Vec::new();

        let heartbeat = tick_stream(Duration::from_secs(1)).fuse();
        // let mut heartbeat = tokio::time::interval(Duration::from_millis(1000));
        let _uptime: u32 = 0;
        let _heartbeat_request_id: u32 = 0;

        futures::pin_mut!(heartbeat);
        loop {
            futures::select! {
                ev = rx_from_instances.select_next_some() => {
                    Self::process_events_from_instances(
                        &ev,
                        &mut filters,
                        &mut remote_nodes
                    ).await;
                }
                ev_int = rx_internal.select_next_some() => {
                    Self::process_internal_events(
                        ev_int,
                        // &mut nodes,
                        &mut filters,
                        &mut remote_nodes
                    ).await;
                }
                // tcp_rx_res = tcp_streams_rx => {
                //     println!("tcp rx: {:?}", tcp_rx_res);
                // }
                // _ = heartbeat.tick() => {
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

                    // for (node_id, sender) in &nodes {
                    //     if sender.is_closed() {
                    //         warn!("Node instance with node id {node_id:?} is down");
                    //     }
                    // }
                    for remote_node in &remote_nodes {
                        if remote_node.to_event_loop.is_closed() {
                            warn!("Remote node attachment to {:?} is down", remote_node.protocol);
                        }
                    }

                    Self::drop_timed_out_filters(&mut filters);
                }
                complete => {
                    warn!("unexpected complete");
                    break;
                }
            }
        }
    }

    async fn process_events_from_instances(
        ev: &AddressableEvent,
        filters: &mut Vec<(EventFilter, Sender<AddressableEvent>)>,
        remote_nodes: &mut Vec<RemoteDescriptor>,
    ) {
        let mut filters_to_drop = vec![];
        let mut forwards_count = 0;
        for (idx, (filter, tx_handle)) in filters.iter_mut().enumerate() {
            if filter.matches(&ev.event) {
                let r = tx_handle.send(ev.clone()).await; // TODO: count
                if r.is_ok() {
                    forwards_count += 1;
                }
                if r.is_err() || filter.is_single_shot() {
                    filters_to_drop.push(idx);
                }
            }
        }
        // trace!("forwarded to {forwards_count} instances");
        for f in filters_to_drop {
            trace!("dropping filter {f}");
            filters.remove(f);
        }

        let mut attachments_addrs = vec![];

        for rd in remote_nodes {
            if rd.protocol == ev.protocol {
                if rd.to_event_loop.send(ev.clone()).await.is_ok() {
                    // trace!("Forwarded to attachment event loop: {:?}", rd.addr);
                    attachments_addrs.push(rd.protocol);
                } else {
                    error!(
                        "Failed to forward event to remote attachment event loop of: {:?}",
                        rd.protocol
                    );
                }
            }
        }
        trace!(
            "rx from instances: {ev:?} -> {forwards_count} instances and -> {attachments_addrs:?}"
        );
    }

    async fn process_internal_events(
        ev: InternalEvent,
        // nodes: &mut HashMap<NodeId, Sender<AddressableEvent>>,
        filters: &mut Vec<(EventFilter, Sender<AddressableEvent>)>,
        remote_nodes: &mut Vec<RemoteDescriptor>,
    ) {
        match ev {
            // InternalEvent::ConnectInstance(id, tx_handle) => {
            //     nodes.insert(id, tx_handle);
            //     info!("connected to {} (executor local)", id.0);
            // }
            // InternalEvent::DisconnectInstance(id) => {
            //     nodes.remove(&id);
            //     info!("disconnected from {}", id.0);
            // }
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
                info!("remote attachment {remote_addr:?} is being dropped");
                todo!()
                // let was_reachable = remote_nodes
                //     .iter()
                //     .filter(|rd| rd.addr == remote_addr)
                //     .map(|rd| rd.reachable.clone())
                //     .next()
                //     .unwrap_or(vec![]);
                // remote_nodes.retain(|rd| rd.addr != remote_addr);

                // // Drop filters that relied on remote node being online
                // let mut dropped_count = 0;
                // filters.retain(|(filter, _)| {
                //     for remote_id in &was_reachable {
                //         if filter.is_waiting_for_node(*remote_id)
                //             && filter.is_drop_on_remote_disconnect()
                //         {
                //             dropped_count += 1;
                //             return false;
                //         }
                //     }
                //     true
                // });
                // if dropped_count != 0 {
                //     debug!(
                //         "{dropped_count} filter(s) was dropped due to remote node going offline"
                //     );
                // }
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

    // async fn connect_instance(&mut self, other: &mut Server) -> Result<(), NodeError> {
    //     self.tx_internal
    //         .send(InternalEvent::ConnectInstance(
    //             other.id,
    //             other.tx_to_event_loop.clone(),
    //         ))
    //         .await?;

    //     Ok(())
    // }

    // async fn disconnect_instance(&mut self, other_node_id: NodeId) -> Result<(), NodeError> {
    //     self.tx_internal
    //         .send(InternalEvent::DisconnectInstance(other_node_id))
    //         .await?;
    //     Ok(())
    // }

    // pub async fn connect_instances(node_a: &mut Self, node_b: &mut Self) -> Result<(), NodeError> {
    //     node_a.connect_instance(node_b).await?;
    //     match node_b.connect_instance(node_a).await {
    //         Ok(_) => Ok(()),
    //         Err(_) => {
    //             node_a.disconnect_instance(node_b.id).await?;
    //             Ok(())
    //         }
    //     }
    // }

    // #[instrument(skip(self), fields(node_id = self.id.0))]
    // pub async fn connect_remote(&mut self, addr: Address) -> Result<(), NodeError> {
    //     let id = self.id;
    //     let to_event_loop = self.tx_to_event_loop.clone();
    //     let to_event_loop_internal = self.tx_internal.clone();

    //     let to_event_loop = match addr.protocol {
    //         Protocol::Tcp { .. } => {
    //             todo!()
    //             // info!("tcp: Connecting to remote {ip_addr}");
    //             // let tcp_stream = TcpStream::connect(ip_addr).await?;
    //             // let codec = RmvlbCodec::new_with_max_length(512); // TODO: do not hardcode
    //             // let (frames_sink, frames_source) = Framed::new(tcp_stream, codec).split();
    //             // info!("Connected");
    //             // let (tx, rx) = mpsc::channel(64);
    //             // tokio::spawn(async move {
    //             //     tcp_event_loop(
    //             //         id,
    //             //         ip_addr,
    //             //         frames_sink,
    //             //         frames_source,
    //             //         to_event_loop.clone(),
    //             //         to_event_loop_internal,
    //             //         rx,
    //             //     )
    //             //     .await
    //             // });
    //             // tx
    //         }
    //         Protocol::Ws { ip_addr, port } => {
    //             let url = format!("ws://{ip_addr}:{port}");
    //             info!("ws: Connecting to remote {url}");
    //             let (ws_stream, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    //             let (frames_sink, frames_source) = ws_stream.split();
    //             info!("connected");
    //             let (tx, rx) = mpsc::channel(64);
    //             tokio::spawn(async move {
    //                 crate::remote::ws::ws_event_loop(
    //                     id,
    //                     addr.protocol,
    //                     frames_sink,
    //                     frames_source,
    //                     to_event_loop.clone(),
    //                     to_event_loop_internal,
    //                     rx,
    //                 )
    //                 .await
    //             });
    //             tx
    //         }
    //     };
    //     let remote_descriptor = RemoteDescriptor {
    //         protocol: addr.protocol,
    //         to_event_loop,
    //     };
    //     self.tx_internal
    //         .send(InternalEvent::ConnectRemote(remote_descriptor))
    //         .await?;
    //     Ok(())
    // }

    #[instrument(skip(self))]
    pub async fn listen(&mut self, protocol: Protocol) -> Result<(), NodeError> {
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
                    super::ws::ws_server_acceptor(listener, tx_to_event_loop, tx_internal).await
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

fn tick_stream(period: Duration) -> impl Stream<Item = ()> {
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
