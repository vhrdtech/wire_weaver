use core::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use futures::channel::mpsc;
use futures::channel::mpsc::{Sender, Receiver};
use futures::{SinkExt, Stream, StreamExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::RwLock;
use crate::node::async_std::error::NodeError;

pub struct VhNode {
    id: u32,
    tx_to_event_loop: Sender<VhLinkEvent>,
    // rx in process_events()
    tx_internal: Sender<InternalEvent>,
    // rx in process_events()
    nodes: Arc<RwLock<HashMap<u32, Option<VhNodeHandle>>>>,
}

struct VhNodeHandle {
    tx: Sender<VhLinkEvent>,
}

impl VhNode {
    /// Create a node with a sole purpose of sending requests to another nodes.
    ///
    /// Created node will contain xPI implementations of: semver, client and will answer to respective
    /// requests. Heartbeats will also be broadcasted.
    pub async fn new_client(id: u32) -> VhNode {
        let (tx_to_even_loop, rx_router) = mpsc::channel(64); // TODO: config
        let (tx_internal, rx_internal) = mpsc::channel(1);
        let nodes = Arc::new(RwLock::new(HashMap::new()));
        let nodes_clone = nodes.clone();
        tokio::spawn(async move {
            Self::process_events(id, rx_router, rx_internal, nodes_clone).await;
        });
        VhNode {
            id,
            tx_to_event_loop: tx_to_even_loop,
            tx_internal,
            nodes,
        }
    }

    // pub async fn new_server
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
        self_id: u32,
        mut rx_from_instances: Receiver<VhLinkEvent>,
        mut rx_internal: Receiver<InternalEvent>,
        nodes_shared: Arc<RwLock<HashMap<u32, Option<VhNodeHandle>>>>,
    ) {
        println!("Node({}) started", self_id);
        // send out heartbeats
        // answer introspects
        // process read/write/subscribe
        // process method calls
        // process timeouts
        // rate shaper?
        let mut nodes: HashMap<u32, VhNodeHandle> = HashMap::new();
        let heartbeat = tick_stream(Duration::from_secs(1)).fuse();
        futures::pin_mut!(heartbeat);
        loop {
            futures::select! {
                ev = rx_from_instances.select_next_some() => {
                    println!("{}: from: {}: ", self_id, ev.from);
                }
                ev_int = rx_internal.select_next_some() => {
                    println!("{}: internal: {:?}", self_id, ev_int);
                    match ev_int {
                        InternalEvent::InstanceConnected(id) => {
                            match nodes_shared.try_write() {
                                Ok(mut nodes_shared) => {
                                    match nodes_shared.get_mut(&id) {
                                        Some(handle) => {
                                            nodes.insert(id, handle.take().unwrap());
                                            println!("{}: taken node {}", self_id, id);
                                        }
                                        None => {
                                            println!("{} internal error: {} not found", self_id, id);
                                        }
                                    }
                                }
                                Err(_) => {
                                    println!("router: failed to acquire read lock for nodes_shared")
                                }
                            }
                        }
                    }
                }
                _ = heartbeat.next() => {
                    println!("{}: local heartbeat", self_id);
                    for (id, handle) in &mut nodes {
                        handle.tx.send(VhLinkEvent { from: self_id }).await;
                    }
                }
                complete => {
                    println!("{}: unexpected complete", self_id)
                }
            }
        }
    }

    async fn connect_instance(&mut self, other: &mut VhNode) -> Result<(), NodeError> {
        let node_handle = VhNodeHandle {
            tx: other.tx_to_event_loop.clone(),
        };
        match self.nodes.try_write() {
            Ok(mut nodes) => {
                if nodes.contains_key(&self.id) {
                    return Err(NodeError::NodeAlreadyAttached(self.id));
                }
                nodes.insert(other.id, Some(node_handle));
            }
            Err(_) => {
                return Err(NodeError::AttachFailed("nodes lock not acquired".to_owned()));
            }
        }
        self.tx_internal.send(InternalEvent::InstanceConnected(other.id))
            .await
            .map_err(|_| NodeError::AttachFailed("tx_internal send() failed".to_owned()))?;

        Ok(())
    }

    pub async fn connect_instances(node_a: &mut Self, node_b: &mut Self) -> Result<(), NodeError> {
        node_a.connect_instance(node_b).await?;
        node_b.connect_instance(node_a).await?; // TODO: if failed, disconnect self
        Ok(())
    }

    // pub async fn connect_remote(addr: NodeAddr) -> Result<VhNode, NodeError> {
    // match addr {
    //     NodeAddr::Tcp(addr) => {
    //         let mut tcp_stream = TcpStream::connect(addr).await?;
    //         let (tcp_rx, tcp_tx) = tcp_stream.into_split();
    //         let (mpsc_tx, mpsc_rx) = mpsc::channel(64); // TODO: move to config
    //         tokio::spawn(async move {
    //             outgoing_process(tcp_tx, mpsc_rx).await;
    //         });
    //         Ok(VhRouter {
    //             mpsc_tx_seed: mpsc_tx,
    //             nodes: HashMap::new()
    //         })
    //     }
    // }
    // todo!()
    // }
}

fn tick_stream(period: Duration) -> impl Stream<Item=()> {
    futures::stream::unfold(period, move |p| {
        async move {
            tokio::time::sleep(period).await;
            Some(((), p))
        }
    })
}

#[derive(Debug, )]
pub struct VhLinkEvent {
    from: u32,
}

#[derive(Debug, )]
enum InternalEvent {
    InstanceConnected(u32),
}

// async fn outgoing_process(mut tcp_tx: OwnedWriteHalf, mut mpsc_rx: Receiver<VhLinkEvent>) {
//     while let Some(ev) = mpsc_rx.next().await {
//         println!("outgoing_process: got event: {:?}", ev);
//         let r = tcp_tx.write_all(&[0x3, 0xa1, 0x16, 0x60, 0x10, 0x52, 0x55, 0x19,  0x0,  0xa,  0x0, 0x14,  0x0,  0x5,  0x0,  0x7,  0x0, 0x1b]).await;
//     }
// }