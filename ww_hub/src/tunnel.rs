//! One client's connection to a device through the hub (SRV-29): a link endpoint that answers like the device (its
//! `DeviceInfo`, `LinkSetup` checked against its user API version by `ww_device`'s `DeviceLink`), requests handed to
//! the device's task, and a queue of what the device's task sends back.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures_lite::future::poll_once;
use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use tokio::sync::Notify;
use tracing::{debug, info, warn};
use wire_weaver::ww_version::{ApiHash, ApiHashPair, CompactVersion, GlobalTypeId};
use wire_weaver_client::DeviceApiInfo;
use wire_weaver_client::in_process::TokioClock;
use ww_device::link::{Received, Transmit};
use ww_device::ww_link::{DisconnectReason, Kind};
use ww_device::{Clock, DeviceLink, LinkConfig, LinkEvent};
use ww_local_socket::{MessageReader, MessageWriter, Peer};

use crate::Hub;
use crate::device::{DeviceHandle, ToDevice};

/// Stream data and property updates queued for one client, in bytes: beyond it they are dropped and counted (a client
/// that doesn't read must not hold up the device or the other clients). Replies and sidebands are never dropped.
pub(crate) const QUEUE_DATA_BYTES: usize = 1024 * 1024;

/// A seq can grow from 1 to 5 bytes on the way to the device: a client is told the device takes this much less.
const SEQ_GROWTH: usize = 4;

/// Link message kind, payload, and whether it counts as droppable data.
type Item = (u8, Vec<u8>, bool);

/// What the device's task sends one client, in order.
pub(crate) struct TunnelQueue {
    inner: Mutex<QueueInner>,
    notify: Notify,
}

#[derive(Default)]
struct QueueInner {
    items: VecDeque<Item>,
    data_bytes: usize,
    dropped: u64,
    /// The device is gone: the client gets `Disconnect` once the items are out
    closed: bool,
    /// ... because it is lent out (SRV-32): the `Disconnect` says so
    lent: bool,
}

impl TunnelQueue {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(TunnelQueue {
            inner: Mutex::new(QueueInner::default()),
            notify: Notify::new(),
        })
    }

    /// An event the client waits for (a reply) or must not miss (a sideband): never dropped.
    pub(crate) fn push(&self, kind: Kind, bytes: Vec<u8>) {
        self.inner
            .lock()
            .unwrap()
            .items
            .push_back((kind as u8, bytes, false));
        self.notify.notify_one();
    }

    /// Stream data or a property update: dropped when the client is [QUEUE_DATA_BYTES] behind.
    pub(crate) fn push_data(&self, bytes: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.data_bytes + bytes.len() > QUEUE_DATA_BYTES {
            inner.dropped += 1;
            if inner.dropped.is_power_of_two() {
                warn!(
                    "ww hub: a client is behind, {} stream events dropped",
                    inner.dropped
                );
            }
            return;
        }
        inner.data_bytes += bytes.len();
        inner.items.push_back((Kind::Data0 as u8, bytes, true));
        drop(inner);
        self.notify.notify_one();
    }

    pub(crate) fn close(&self) {
        self.inner.lock().unwrap().closed = true;
        self.notify.notify_one();
    }

    /// [Self::close], the device lent to a program outside the hub: the client's `Disconnect` carries
    /// [ww_local_socket::DISCONNECT_LENT].
    pub(crate) fn close_lent(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        inner.lent = true;
        drop(inner);
        self.notify.notify_one();
    }

    /// Stream events dropped so far.
    pub(crate) fn dropped(&self) -> u64 {
        self.inner.lock().unwrap().dropped
    }

    /// What is queued, and the reason to disconnect with once it is out if the device is gone.
    fn take(&self) -> (Vec<Item>, Option<DisconnectReason>) {
        let mut inner = self.inner.lock().unwrap();
        inner.data_bytes = 0;
        let items = inner.items.drain(..).collect();
        let closed = inner.closed.then_some(match inner.lent {
            true => DisconnectReason::Other(ww_local_socket::DISCONNECT_LENT),
            false => DisconnectReason::RequestByUser,
        });
        (items, closed)
    }
}

/// The device's link versions and hash, as its `DeviceInfo` says, for a client's link endpoint.
pub(crate) struct DeviceLinkInfo {
    api: DeviceApiInfo,
    model: CompactVersion,
}

impl DeviceLinkInfo {
    pub(crate) fn new(api: DeviceApiInfo) -> Self {
        // the event loop reports the compact API model version as `G<id>`
        let m = &api.api_model_version;
        let id = m
            .crate_id
            .strip_prefix('G')
            .and_then(|id| id.parse().ok())
            .unwrap_or(0);
        let model = CompactVersion::new(
            GlobalTypeId::new(id),
            m.version.major.0,
            m.version.minor.0,
            m.version.patch.0,
        );
        DeviceLinkInfo { api, model }
    }

    fn link_config(&self) -> LinkConfig<'_> {
        let hash = &self.api.user_api_hash;
        let mut config = LinkConfig::new(
            self.api.user_api_version.as_ref(),
            ApiHashPair {
                no_docs: ApiHash {
                    hash: wire_weaver::shrink_wrap::RefVec::Slice {
                        slice: &hash.no_docs.hash,
                    },
                },
                with_docs: ApiHash {
                    hash: wire_weaver::shrink_wrap::RefVec::Slice {
                        slice: &hash.with_docs.hash,
                    },
                },
            },
            self.model,
        );
        // the device's own: a client batches as it would talk to the device directly, and the hub sends each batch
        // on as the client flushed it
        config.accumulation_time = self.api.accumulation_time;
        config
    }

    /// What a client may send: what the device takes, less room for a longer seq.
    fn max_message_len(&self) -> usize {
        self.api.max_message_size.saturating_sub(SEQ_GROWTH)
    }
}

/// Serves one client's tunnel to `device` until either side leaves.
pub(crate) async fn serve(
    hub: Arc<Hub>,
    id: u32,
    device: DeviceHandle,
    queue: Arc<TunnelQueue>,
    rx: RecvHalf,
    tx: SendHalf,
    peer: Peer,
) {
    let clock = TokioClock::new();
    let mut link = DeviceLink::new(device.info.link_config(), device.info.max_message_len());
    link.on_transport_up(clock.now());
    let mut reader = MessageReader::new(rx, device.info.api.max_message_size + 1);
    let mut writer = MessageWriter::new(tx);
    let mut scratch = vec![0u8; 512];
    let mut device_gone = false;
    loop {
        let deadline = link.poll_timeout();
        let timer = async {
            match deadline {
                Some(at) => clock.wait_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            r = reader.wait_message() => match r {
                Ok(()) => {
                    let (kind, payload) = reader.message().expect("a message was just received");
                    let received = match link.handle_message(clock.now(), kind, payload) {
                        Some(Received::Data(bytes)) => Some(Err(bytes.to_vec())),
                        Some(Received::Loopback { repeat, seq, data }) => {
                            Some(Ok((repeat, seq, data.to_vec())))
                        }
                        None => None,
                    };
                    reader.consume();
                    // the end of what the client wrote at once: it flushed, the device gets it now as well
                    let last = received.is_some() && poll_once(reader.wait_message()).await.is_none();
                    let sent = match received {
                        Some(Err(request)) => device.device.request(id, request, last).await,
                        Some(Ok((repeat, seq, data))) => {
                            device.device.loopback(id, repeat, seq, data, last).await
                        }
                        None => Ok(()),
                    };
                    if sent.is_err() {
                        // the device's event loop is gone; its task closes the queue
                        debug!("ww hub: tunnel {id}: device gone");
                    }
                }
                Err(e) => {
                    debug!("ww hub: tunnel {id} ({peer:?}) closed: {e}");
                    break;
                }
            },
            _ = queue.notify.notified() => {
                let (items, closed) = queue.take();
                for (kind, bytes, _) in items {
                    if kind == Kind::Data0 as u8 && link.check_send(bytes.len()).is_err() {
                        debug!("ww hub: tunnel {id}: event of {} bytes not sent", bytes.len());
                        continue;
                    }
                    writer.push(kind, &bytes);
                }
                if let Some(reason) = closed {
                    device_gone = true;
                    // lent (SRV-32) says so; else the hub may be handing over (SRV-30): the client reconnects
                    // to the next one
                    link.disconnect(match reason {
                        DisconnectReason::RequestByUser => hub.closing_reason(),
                        lent => lent,
                    });
                }
            },
            _ = timer => link.handle_timeout(clock.now()),
        }
        match link.poll_event() {
            Some(LinkEvent::Up) => hub.client_up(id, link.peer_meta().cloned()),
            Some(LinkEvent::Down(reason)) => {
                debug!("ww hub: tunnel {id} link down: {reason:?}");
                // the Disconnect, if any, still goes out below
                device_gone = true;
            }
            None => {}
        }
        while let Some(t) = link.poll_transmit(clock.now(), &mut scratch) {
            match t {
                Transmit::NewSession => writer.clear(),
                Transmit::Message { kind, bytes } => writer.push(kind, bytes),
                Transmit::Flush => {}
            }
        }
        if writer.flush().await.is_err() || device_gone {
            break;
        }
    }
    let _ = writer.shutdown().await;
    let dropped = queue.dropped();
    if dropped > 0 {
        info!(
            "ww hub: tunnel {id} ({peer:?}) dropped {dropped} stream events, the client was behind"
        );
    }
    let _ = device.tx.send(ToDevice::Leave { id });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stream data beyond the limit is dropped and counted, replies and sidebands never, and order is kept.
    #[test]
    fn queue_drops_data_not_replies() {
        let q = TunnelQueue::new();
        q.push_data(vec![0; QUEUE_DATA_BYTES - 10]);
        q.push_data(vec![1; 20]);
        q.push(Kind::Data0, vec![2; 100]);
        q.push_data(vec![3; 5]);
        assert_eq!(q.dropped(), 1);
        let (items, closed) = q.take();
        let firsts: Vec<u8> = items.iter().map(|(_, b, _)| b[0]).collect();
        assert_eq!(firsts, [0, 2, 3]);
        assert_eq!(closed, None);
        // taken: room again
        q.push_data(vec![4; QUEUE_DATA_BYTES]);
        assert_eq!(q.dropped(), 1);
        q.close();
        assert_eq!(q.take().1, Some(DisconnectReason::RequestByUser));
        let q = TunnelQueue::new();
        q.close_lent();
        assert_eq!(
            q.take().1,
            Some(DisconnectReason::Other(ww_local_socket::DISCONNECT_LENT))
        );
    }
}
