use shrink_wrap::vec::RefVec;
use shrink_wrap::{BufWriter, SerializeShrinkWrap};
use std::net::IpAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, info, trace};
use wire_weaver_client_server::event_loop_state::CommonState;
use wire_weaver_client_server::ww::no_alloc_client::client_server_v0_1;
use wire_weaver_client_server::ww::no_alloc_client::client_server_v0_1::RequestKind;
use wire_weaver_client_server::{Command, Error};

pub struct UdpTarget {
    pub addr: IpAddr,
    pub port: u16,
}

#[derive(thiserror::Error, Debug)]
pub enum UdpError {
    #[error("test")]
    Test,
}

struct State {
    common: CommonState<UdpError>,
}

impl Default for State {
    fn default() -> Self {
        State {
            common: CommonState::default(),
        }
    }
}

struct Link {
    target: UdpTarget,
    socket: UdpSocket,
}

pub async fn udp_worker(mut cmd_rx: mpsc::UnboundedReceiver<Command<UdpTarget, UdpError>>) {
    debug!("udp worker started");
    let mut state = State::default();
    let mut link = None;
    let mut scratch = [0u8; 2048];
    loop {
        let Some(cmd) = cmd_rx.recv().await else {
            debug!("udp worker exiting, because all command senders were dropped");
            break;
        };

        match cmd {
            Command::Connect {
                filter,
                on_error,
                connected_tx,
            } => {
                // TODO: send connection request, handle timeout, wait response
                let udp_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
                let local_addr = udp_socket.local_addr().unwrap();
                info!("local addr: {}", local_addr);
                link = Some(Link {
                    target: filter,
                    socket: udp_socket,
                });
                if let Some(tx) = connected_tx {
                    tx.send(Ok(())).unwrap();
                }
            }
            Command::DisconnectKeepStreams { disconnected_tx } => {
                info!("Disconnecting on user request (but keeping streams ready for re-use)");
                if let Some(tx) = disconnected_tx {
                    _ = tx.send(());
                }
            }
            Command::DisconnectAndExit { disconnected_tx } => {
                info!("Disconnecting and stopping UDP event loop on user request");
                if let Some(tx) = disconnected_tx {
                    _ = tx.send(());
                }
                break;
            }
            Command::SendCall {
                args_bytes,
                path,
                timeout,
                done_tx,
            } => {
                trace!("sending call to {path:?}");
                let seq = state.common.register_prune_next_seq(timeout, done_tx);
                let request = client_server_v0_1::Request {
                    seq,
                    path: RefVec::Slice { slice: &path },
                    kind: RequestKind::Call {
                        args: RefVec::new_bytes(&args_bytes),
                    },
                };
                if let Some(link) = &mut link {
                    serialize_request_send(request, link, &mut state, &mut scratch).await;
                }
            }
            Command::SendWrite {
                value_bytes,
                path,
                timeout,
                done_tx,
            } => {
                trace!("sending write to {path:?}");
                let seq = state.common.register_prune_next_seq(timeout, done_tx);
                let request = client_server_v0_1::Request {
                    seq,
                    path: RefVec::Slice { slice: &path },
                    kind: RequestKind::Write {
                        data: RefVec::new_bytes(&value_bytes),
                    },
                };
                if let Some(link) = &mut link {
                    serialize_request_send(request, link, &mut state, &mut scratch).await;
                }
            }
            Command::SendRead { .. } => {}
            Command::Subscribe { .. } => {}
        }
    }
    state.common.cancel_all_streams();
    state.common.cancel_all_requests();
    debug!("udp worker exited");
}

async fn serialize_request_send(
    request: client_server_v0_1::Request<'_>,
    link: &mut Link,
    _state: &mut State,
    scratch: &mut [u8],
) -> Result<(), Error<UdpError>> {
    let mut wr = BufWriter::new(scratch);
    request.ser_shrink_wrap(&mut wr).unwrap();
    let request_bytes = wr.finish_and_take().unwrap().to_vec();

    let r = link
        .socket
        .send_to(
            request_bytes.as_slice(),
            (link.target.addr, link.target.port),
        )
        .await;
    info!("sending request to {} {r:?}", link.target.addr);
    // if link.is_tx_queue_empty() {
    //     state.common.packet_started_instant = None;
    // } else {
    //     state.common.packet_started_instant = Some(Instant::now());
    // }
    // link.force_send().await?; // TODO: force send on timer
    Ok(())
}
