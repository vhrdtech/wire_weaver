use core::time::Duration;
use futures::channel::mpsc;
use futures::channel::mpsc::{Receiver, Sender};
use futures::{SinkExt, Stream, StreamExt};
use std::collections::HashMap;
use tokio::io::AsyncReadExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use futures::FutureExt;
use crate::node::addressing::RemoteNodeAddr;
use crate::node::async_std::NodeError;
use xpi::xpi_owned::{NodeId, XpiEventOwned, XpiEventKind, Priority, NodeSet};

pub struct VhNode {
    id: NodeId,
    tx_to_event_loop: Sender<XpiEventOwned>,
    tx_internal: Sender<InternalEvent>,
}

fn read_many(rx: OwnedReadHalf) -> impl Stream<Item=std::io::Result<Vec<u8>>> {
    futures::stream::unfold(rx, |mut rx| {
        async {
            // let mut data = None;
            // loop {
            //     futures::select! {
            //         read = read_one(&mut rx).fuse() => {
            //             println!("Datagram received: ", );
            //             break;
            //         },
            //         complete => break,
            //     }
            // };
            let data = read_one(&mut rx).await;
            Some(( data, rx))
        }
    })
}

async fn read_one(rx: &mut OwnedReadHalf) -> std::io::Result<Vec<u8>> {
    let mut data = vec![0; 1024];
    let len = rx.read(&mut data).await?;
    data.truncate(len);
    Ok(data)
}

impl VhNode {
    /// Create a node with a sole purpose of sending requests to another nodes.
    ///
    /// Created node will contain xPI implementations of: semver, client and will answer to respective
    /// requests. Heartbeats will also be broadcasted.
    pub async fn new_client(id: NodeId /* xPI client, generated or dynamically loaded */) -> VhNode {
        let (tx_to_even_loop, rx_router) = mpsc::channel(64); // TODO: config
        let (tx_internal, rx_internal) = mpsc::channel(1);
        tokio::spawn(async move {
            Self::process_events(id, rx_router, rx_internal).await;
        });
        VhNode {
            id,
            tx_to_event_loop: tx_to_even_loop,
            tx_internal,
            // nodes,
        }
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
    async fn process_events(
        self_id: NodeId,
        mut rx_from_instances: Receiver<XpiEventOwned>,
        mut rx_internal: Receiver<InternalEvent>,
    ) {
        println!("Node({}) started", self_id.0);
        // send out heartbeats
        // answer introspects
        // process read/write/subscribe
        // process method calls
        // process timeouts
        // rate shaper?

        // tx handles to another node instances running on the same executor
        let mut nodes: HashMap<NodeId, Sender<XpiEventOwned>> = HashMap::new();

        // tx handles to another nodes running on remote machines or in another processes
        let mut remote_nodes: HashMap<u32, Sender<XpiEventOwned>> = HashMap::new();

        // tx handles to Self for filter_one and filter_many
        let mut filters: HashMap<u32, Sender<XpiEventOwned>> = HashMap::new();

        let heartbeat = tick_stream(Duration::from_secs(1)).fuse();
        // let mut heartbeat = tokio::time::interval(Duration::from_millis(1000));
        let mut uptime: u32 = 0;

        futures::pin_mut!(heartbeat);
        loop {
            futures::select! {
                ev = rx_from_instances.select_next_some() => {
                    println!("{}: {:?}", self_id.0, ev);
                    let mut filters_to_drop = vec![];
                    for (_filter, tx_handle) in &mut filters {
                        // if _filter.matches
                        let _ = tx_handle.send(ev.clone()).await; // TODO: count
                        filters_to_drop.push(0);
                    }
                    for f in filters_to_drop {
                        filters.remove(&f);
                    }
                }
                ev_int = rx_internal.select_next_some() => {
                    match ev_int {
                        InternalEvent::ConnectInstance(id, tx_handle) => {
                            nodes.insert(id, tx_handle);
                            println!("{}: connected to {} (executor local)", self_id.0, id.0);
                        }
                        InternalEvent::FilterOne(_filter, tx_handle) => {
                            println!("filter registered");
                            filters.insert(0, tx_handle);
                        }
                        InternalEvent::ConnectRemoteTcp(tx_handle) => {
                            remote_nodes.insert(0, tx_handle);
                        }
                    }
                }
                // tcp_rx_res = tcp_streams_rx => {
                //     println!("tcp rx: {:?}", tcp_rx_res);
                // }
                // _ = heartbeat.tick() => {
                _ = heartbeat.next() => {
                    println!("{}: local heartbeat", self_id.0);
                    for (_id, tx_handle) in &mut nodes {
                        // let _r = handle.tx.send(VhLinkEvent { from: self_id }).await; // TODO: handle error
                        tx_handle.send(XpiEventOwned::new(self_id, NodeSet::Broadcast, XpiEventKind::new_heartbeat(uptime), Priority::Lossy(0))).await;
                    }
                    uptime += 1;
                }
                complete => {
                    println!("{}: unexpected complete", self_id.0);
                    break;
                }
            }
        }
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

    pub async fn connect_instances(node_a: &mut Self, node_b: &mut Self) -> Result<(), NodeError> {
        node_a.connect_instance(node_b).await?;
        node_b.connect_instance(node_a).await?; // TODO: if failed, disconnect self
        Ok(())
    }

    pub async fn connect_remote(&mut self, addr: RemoteNodeAddr) -> Result<(), NodeError> {
        match addr {
            RemoteNodeAddr::Tcp(addr) => {
                let mut tcp_stream = TcpStream::connect(addr).await?;
                let (tx, rx) = mpsc::channel(64);
                let id = self.id.clone();
                let to_event_loop = self.tx_to_event_loop.clone();
                tokio::spawn(async move {
                    Self::tcp_event_loop(
                        id,
                        tcp_stream,
                        to_event_loop.clone(),
                        rx
                    ).await
                });
                self.tx_internal.send(InternalEvent::ConnectRemoteTcp(tx)).await?;
                Ok(())
            }
        }
    }

    async fn tcp_event_loop(
        self_id: NodeId,
        mut stream: TcpStream,
        to_event_loop: Sender<XpiEventOwned>,
        mut from_event_loop: Receiver<XpiEventOwned>,
    ) {
        let (mut tcp_rx, mut tcp_tx) = stream.split();
        let mut buf = [0u8; 10_000];
        loop {
            futures::select! {
                len = tcp_rx.read(&mut buf).fuse() => {
                    println!("tcp rx: {:?}", len);
                }
                ev = from_event_loop.select_next_some() => {

                }
            }
        }
    }

    /// Send event to the event loop and return immediately. Event will be send to another node or nodes
    /// directly or through one of the interfaces available depending on the destination.
    pub async fn submit_one(&mut self, ev: XpiEventOwned) -> Result<(), NodeError> {
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
    pub async fn filter_one(&mut self, filter: ()) -> Result<XpiEventOwned, NodeError> {
        let (tx, mut rx) = mpsc::channel(1);
        self.tx_internal
            .send(InternalEvent::FilterOne(
                (),
                tx,
            ))
            .await?;
        let ev = rx.next().await.ok_or(NodeError::FilterOneFail)?;
        Ok(ev)
    }

    /// Get a stream source with only the desired events in it.
    /// For subscribing to property updates and streams.
    pub async fn filter_many(&mut self, _ev: XpiEventOwned) -> u32 {
        todo!()
    }
}

fn tick_stream(period: Duration) -> impl Stream<Item=()> {
    futures::stream::unfold(period, move |p| async move {
        tokio::time::sleep(period).await;
        Some(((), p))
    })
}

#[derive(Debug)]
enum InternalEvent {
    ConnectInstance(NodeId, Sender<XpiEventOwned>),
    ConnectRemoteTcp(Sender<XpiEventOwned>),
    FilterOne((), Sender<XpiEventOwned>),
}

// async fn outgoing_process(mut tcp_tx: OwnedWriteHalf, mut mpsc_rx: Receiver<VhLinkEvent>) {
//     while let Some(ev) = mpsc_rx.next().await {
//         println!("outgoing_process: got event: {:?}", ev);
//         let r = tcp_tx.write_all(&[0x3, 0xa1, 0x16, 0x60, 0x10, 0x52, 0x55, 0x19,  0x0,  0xa,  0x0, 0x14,  0x0,  0x5,  0x0,  0x7,  0x0, 0x1b]).await;
//     }
// }
