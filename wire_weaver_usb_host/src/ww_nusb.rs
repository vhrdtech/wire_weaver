//! Handle packet sending and receiving between nusb and wire_weaver_usb_link

use nusb::transfer::{
    Buffer, Bulk, BulkOrInterrupt, Completion, EndpointDirection, In, Interrupt, Out, TransferError,
};
use nusb::{Endpoint, Interface};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, error, trace, warn};
use wire_weaver_usb_link::{PacketSink, PacketSource};

pub(crate) struct Sink {
    buf_pool: Vec<Buffer>,
    submit_tx: mpsc::Sender<Buffer>,
    completion_rx: mpsc::Receiver<Completion>,
    max_packet_size: usize,
    marker: &'static str,
}

pub(crate) const ERR_WRITE_PACKET_TIMEOUT: u32 = u32::MAX; // TODO: try to replace TransferError with own type to avoid piggy-backing on nusb error

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
        let (submit_tx, submit_rx) = mpsc::channel(8);
        let (completion_tx, completion_rx) = mpsc::channel(8);
        let buf_pool = vec![
            ep_out.allocate(max_packet_size),
            ep_out.allocate(max_packet_size),
        ];
        tokio::spawn(async move {
            endpoint_worker(ep_out, submit_rx, completion_tx, marker).await;
        });
        Ok(Sink {
            buf_pool,
            submit_tx,
            completion_rx,
            max_packet_size,
            marker,
        })
    }
}

impl PacketSink for Sink {
    type Error = TransferError;

    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let mut buf = if let Some(buf) = self.buf_pool.pop() {
            buf
        } else {
            match timeout(Duration::from_millis(500), self.completion_rx.recv()).await {
                Ok(Some(completion)) => {
                    completion.status?; // return early if transfer failed
                    completion.buffer
                }
                Ok(None) => return Err(TransferError::Disconnected),
                // If device boots, but then ends up in an endless loop or in HardFault, USB device is still detected by the host,
                // but no transfers go through. I.e. USB peripheral continues to answer to host requests, so it appears connected.
                // Ideally device should reset USB its peripheral (or otherwise cause it to stop) in HardFault to drop from the bus.
                Err(_) => return Err(TransferError::Unknown(ERR_WRITE_PACKET_TIMEOUT)),
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
        // theoretically, can use obtained buffer inside a closure, to fill it without copying
        buf.extend_from_slice(data);
        if self.submit_tx.send(buf).await.is_err() {
            warn!("{}: submit channel dropped", self.marker);
            return Err(TransferError::Disconnected);
        }
        trace!("submitted packet: {}: {:02x?}", data.len(), data);
        Ok(())
    }
}

// impl Sink {
// Wait for all previously submitted transfer to be actually completed
// Does not really work, if interface is dropped, transfer don't make it through
// pub async fn flush(&mut self) -> Result<(), TransferError> {
//     loop {
//         match self.completion_rx.try_recv() {
//             Ok(completion) => {
//                 println!("flush got one");
//                 completion.status?; // error out on first previous transfer error
//                 self.buf_pool.push(completion.buffer);
//             }
//             Err(TryRecvError::Empty) => break Ok(()),
//             Err(TryRecvError::Disconnected) => break Err(TransferError::Disconnected),
//         }
//     }
// }
// }

async fn endpoint_worker<EpType: BulkOrInterrupt, Dir: EndpointDirection>(
    mut ep: Endpoint<EpType, Dir>,
    mut submit_rx: mpsc::Receiver<Buffer>,
    completion_tx: mpsc::Sender<Completion>,
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
}

pub(crate) struct Source {
    submit_tx: mpsc::Sender<Buffer>,
    completion_rx: mpsc::Receiver<Completion>,
    marker: &'static str,
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
        for _ in 0..2 {
            let mut rx = ep_in.allocate(max_packet_size);
            rx.set_requested_len(max_packet_size);
            ep_in.submit(rx);
        }
        let (submit_tx, submit_rx) = mpsc::channel(8);
        let (completion_tx, completion_rx) = mpsc::channel(8);
        tokio::spawn(async move {
            endpoint_worker(ep_in, submit_rx, completion_tx, marker).await;
        });
        Ok(Source {
            submit_tx,
            completion_rx,
            marker,
        })
    }
}

impl PacketSource for Source {
    type Error = TransferError;

    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
        match self.completion_rx.recv().await {
            Some(completion) => {
                completion.status?;
                let buf = completion.buffer;
                let len = buf.len();
                data[..len].copy_from_slice(&buf);
                trace!("received packet: {}: {:02x?}", len, &data[..len]);
                if self.submit_tx.send(buf).await.is_err() {
                    warn!("{}: submit channel dropped", self.marker);
                    Err(TransferError::Disconnected)
                } else {
                    Ok(len)
                }
            }
            None => Err(TransferError::Disconnected),
        }
    }
}
