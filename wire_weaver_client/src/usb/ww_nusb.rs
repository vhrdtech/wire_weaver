//! Handle packet sending and receiving between nusb and the event loop

use nusb::transfer::{
    Buffer, Bulk, BulkOrInterrupt, Completion, EndpointDirection, In, Interrupt, Out, TransferError,
};
use nusb::{Endpoint, Interface};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};
use tracing::{debug, error, trace, warn};

pub(crate) struct Sink {
    buf_pool: Vec<Buffer>,
    submit_tx: mpsc::Sender<Buffer>,
    completion_rx: mpsc::Receiver<Completion>,
    max_packet_size: usize,
    marker: &'static str,
    worker: JoinHandle<()>,
}

// Rx and tx are used from two independent tasks, so a slow device can never stall receiving.
const TX_QUEUE_SIZE: usize = 4;
const RX_QUEUE_SIZE: usize = 64;
/// On close, how long to wait for queued OUT packets (e.g., the final Disconnect) to go out, before cancelling them.
const TX_LINGER: Duration = Duration::from_millis(100);
/// How long to wait for cancelled transfers to be returned by the OS.
const CANCEL_TIMEOUT: Duration = Duration::from_secs(1);

impl Sink {
    pub fn new(
        interface: &Interface,
        max_packet_size: usize,
        use_bulk: bool,
    ) -> Result<Self, nusb::Error> {
        if use_bulk {
            Self::new_inner::<Bulk>(interface, max_packet_size, "bulk_out")
        } else {
            Self::new_inner::<Interrupt>(interface, max_packet_size, "irq_out")
        }
    }

    fn new_inner<EpType: BulkOrInterrupt + 'static>(
        interface: &Interface,
        max_packet_size: usize,
        marker: &'static str,
    ) -> Result<Self, nusb::Error> {
        let ep_out = interface.endpoint::<EpType, Out>(0x01)?; // TODO: un-hardcode endpoint addresses
        let (submit_tx, submit_rx) = mpsc::channel(TX_QUEUE_SIZE);
        let (completion_tx, completion_rx) = mpsc::channel(TX_QUEUE_SIZE);
        let mut buf_pool = Vec::with_capacity(TX_QUEUE_SIZE);
        for _ in 0..TX_QUEUE_SIZE {
            buf_pool.push(ep_out.allocate(max_packet_size));
        }
        let worker = tokio::spawn(async move {
            endpoint_worker(ep_out, submit_rx, completion_tx, TX_LINGER, marker).await;
        });
        Ok(Sink {
            buf_pool,
            submit_tx,
            completion_rx,
            max_packet_size,
            marker,
            worker,
        })
    }
}

impl Sink {
    /// Give already submitted packets a chance to go out, then release the endpoint.
    pub async fn close(self) {
        let Sink {
            submit_tx,
            completion_rx,
            worker,
            ..
        } = self;
        drop(submit_tx);
        drop(completion_rx);
        _ = worker.await;
    }

    /// Waits for a free transfer buffer when all are in flight. Never times out on its own —
    /// the event loop's peer timeout decides when a device is gone.
    /// Not cancel-safe: do not use inside `select!`.
    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), TransferError> {
        let mut buf = if let Some(buf) = self.buf_pool.pop() {
            buf
        } else {
            match self.completion_rx.recv().await {
                Some(completion) => {
                    if let Err(e) = completion.status {
                        self.buf_pool.push(completion.buffer);
                        return Err(e);
                    }
                    completion.buffer
                }
                None => return Err(TransferError::Disconnected),
            }
        };
        buf.clear();
        if data.len() > buf.capacity() {
            error!(
                "{}: tried transmitting packet with length: {}, max: {}",
                self.marker,
                data.len(),
                self.max_packet_size
            );
            self.buf_pool.push(buf);
            return Err(TransferError::InvalidArgument);
        }
        buf.extend_from_slice(data);
        // exactly TX_QUEUE_SIZE buffers exist and the channel holds TX_QUEUE_SIZE, so this never fills
        match self.submit_tx.try_send(buf) {
            Ok(()) => {
                trace!("submitted packet: {}: {:02x?}", data.len(), data);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(buf)) => {
                self.buf_pool.push(buf);
                Err(TransferError::Unknown(0))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("{}: submit channel dropped", self.marker);
                Err(TransferError::Disconnected)
            }
        }
    }
}

async fn endpoint_worker<EpType: BulkOrInterrupt, Dir: EndpointDirection>(
    mut ep: Endpoint<EpType, Dir>,
    mut submit_rx: mpsc::Receiver<Buffer>,
    completion_tx: mpsc::Sender<Completion>,
    linger: Duration,
    marker: &'static str,
) {
    loop {
        if ep.pending() > 0 {
            tokio::select! {
                buf = submit_rx.recv() => {
                    let Some(buf) = buf else {
                        debug!("{marker}: submit channel closed, exiting");
                        break;
                    };
                    ep.submit(buf);
                }
                completion = ep.next_complete() => {
                    let r = completion_tx.send(completion).await;
                    if r.is_err() {
                        debug!("{marker}: completion channel closed, exiting");
                        break;
                    }
                }
            }
        } else if let Some(buf) = submit_rx.recv().await {
            ep.submit(buf);
        } else {
            debug!("{marker}: submit channel closed, exiting");
            break;
        }
    }
    drain(&mut ep, linger, marker).await;
}

/// Wait up to `linger` for pending transfers to complete, then cancel the rest and wait for them to be returned.
/// Cancelled, but not yet returned transfers keep the interface claimed even after the endpoint is dropped,
/// and re-connecting right away fails with "interface is busy".
async fn drain<EpType: BulkOrInterrupt, Dir: EndpointDirection>(
    ep: &mut Endpoint<EpType, Dir>,
    linger: Duration,
    marker: &'static str,
) {
    let deadline = Instant::now() + linger;
    while ep.pending() > 0 {
        if timeout_at(deadline, ep.next_complete()).await.is_err() {
            break;
        }
    }
    if ep.pending() == 0 {
        return;
    }
    trace!("{marker}: cancelling {} pending transfers", ep.pending());
    ep.cancel_all();
    while ep.pending() > 0 {
        if timeout(CANCEL_TIMEOUT, ep.next_complete()).await.is_err() {
            warn!(
                "{marker}: {} cancelled transfers were not returned in time",
                ep.pending()
            );
            return;
        }
    }
}

pub(crate) struct Source {
    submit_tx: mpsc::Sender<Buffer>,
    completion_rx: mpsc::Receiver<Completion>,
    marker: &'static str,
    worker: JoinHandle<()>,
}

impl Source {
    pub fn new(
        interface: &Interface,
        max_packet_size: usize,
        use_bulk: bool,
    ) -> Result<Self, nusb::Error> {
        if use_bulk {
            Self::new_inner::<Bulk>(interface, max_packet_size, "bulk_in")
        } else {
            Self::new_inner::<Interrupt>(interface, max_packet_size, "irq_in")
        }
    }

    fn new_inner<EpType: BulkOrInterrupt + 'static>(
        interface: &Interface,
        max_packet_size: usize,
        marker: &'static str,
    ) -> Result<Self, nusb::Error> {
        let mut ep_in = interface.endpoint::<EpType, In>(0x81)?;
        for _ in 0..RX_QUEUE_SIZE {
            let mut rx = ep_in.allocate(max_packet_size);
            rx.set_requested_len(max_packet_size);
            ep_in.submit(rx);
        }
        let (submit_tx, submit_rx) = mpsc::channel(RX_QUEUE_SIZE);
        let (completion_tx, completion_rx) = mpsc::channel(RX_QUEUE_SIZE);
        let worker = tokio::spawn(async move {
            // IN transfers only complete when a device sends something, no point waiting for them
            endpoint_worker(ep_in, submit_rx, completion_tx, Duration::ZERO, marker).await;
        });
        Ok(Source {
            submit_tx,
            completion_rx,
            marker,
            worker,
        })
    }
}

impl Source {
    /// Cancel all pending transfers and release the endpoint.
    pub async fn close(self) {
        let Source {
            submit_tx,
            completion_rx,
            worker,
            ..
        } = self;
        drop(submit_tx);
        drop(completion_rx);
        _ = worker.await;
    }

    // /// Cancel-safe: the only await is on the completion channel.
    // pub async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, TransferError> {
    //     let mut len = 0;
    //     self.read_packet_with(|p| {
    //         len = p.len();
    //         data[..p.len()].copy_from_slice(p);
    //     })
    //     .await?;
    //     Ok(len)
    // }

    /// Cancel-safe: the only await is on the completion channel.
    pub async fn read_packet_with<F: FnMut(&[u8])>(
        &mut self,
        mut f: F,
    ) -> Result<(), TransferError> {
        match self.completion_rx.recv().await {
            Some(completion) => {
                let buf = completion.buffer;
                let len = buf.len();
                let status = completion.status;
                if status.is_ok() {
                    trace!("received packet: {}: {:02x?}", len, &buf[..len]);
                    f(&buf[..len]);
                }
                // resubmit even on error, so that rx keeps flowing; exactly RX_QUEUE_SIZE buffers exist
                if self.submit_tx.try_send(buf).is_err() {
                    warn!("{}: submit channel dropped or full", self.marker);
                    return Err(TransferError::Disconnected);
                }
                status?;
                Ok(())
            }
            None => Err(TransferError::Disconnected),
        }
    }
}
