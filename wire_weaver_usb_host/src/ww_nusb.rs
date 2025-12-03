//! Handle packet sending and receiving between nusb and wire_weaver_usb_link
use nusb::transfer::{
    Buffer, BulkOrInterrupt, Completion, EndpointDirection, In, Interrupt, Out, TransferError,
};
use nusb::{Endpoint, Interface};
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};
use wire_weaver_usb_link::{PacketSink, PacketSource};

pub(crate) struct Sink {
    buf_pool: Vec<Buffer>,
    submit_tx: mpsc::Sender<Buffer>,
    completion_rx: mpsc::Receiver<Completion>,
    irq_max_packet_size: usize,
}

impl Sink {
    pub fn new(interface: &Interface, irq_max_packet_size: usize) -> Result<Self, nusb::Error> {
        let irq_out = interface.endpoint::<Interrupt, Out>(0x01)?; // TODO: un-hardcode endpoint addresses
        let (submit_tx, submit_rx) = mpsc::channel(8);
        let (completion_tx, completion_rx) = mpsc::channel(8);
        let buf_pool = vec![
            irq_out.allocate(irq_max_packet_size),
            irq_out.allocate(irq_max_packet_size),
        ];
        tokio::spawn(async move {
            endpoint_worker(irq_out, submit_rx, completion_tx, "irq_out").await;
        });
        Ok(Sink {
            buf_pool,
            submit_tx,
            completion_rx,
            irq_max_packet_size,
        })
    }
}

impl PacketSink for Sink {
    type Error = TransferError;

    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let mut buf = if let Some(buf) = self.buf_pool.pop() {
            buf
        } else {
            match self.completion_rx.recv().await {
                Some(completion) => {
                    completion.status?; // return early if transfer failed
                    completion.buffer
                }
                None => return Err(TransferError::Disconnected),
            }
        };
        buf.clear();
        if data.len() > buf.capacity() {
            error!(
                "irq_out: tried transmitting packet with length: {}, max: {}",
                data.len(),
                self.irq_max_packet_size
            );
            self.buf_pool.push(buf);
            return Err(TransferError::InvalidArgument);
        }
        // theoretically, can use obtained buffer inside a closure, to fill it without copying
        buf.extend_from_slice(data);
        if self.submit_tx.send(buf).await.is_err() {
            warn!("irq_out: submit channel dropped");
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
}

impl Source {
    pub fn new(interface: &Interface, irq_max_packet_size: usize) -> Result<Self, nusb::Error> {
        let mut irq_in = interface.endpoint::<Interrupt, In>(0x81)?;
        for _ in 0..2 {
            let mut rx = irq_in.allocate(irq_max_packet_size);
            rx.set_requested_len(irq_max_packet_size);
            irq_in.submit(rx);
        }
        let (submit_tx, submit_rx) = mpsc::channel(8);
        let (completion_tx, completion_rx) = mpsc::channel(8);
        tokio::spawn(async move {
            endpoint_worker(irq_in, submit_rx, completion_tx, "irq_in").await;
        });
        Ok(Source {
            submit_tx,
            completion_rx,
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
                    warn!("irq_out: submit channel dropped");
                    Err(TransferError::Disconnected)
                } else {
                    Ok(len)
                }
            }
            None => Err(TransferError::Disconnected),
        }
    }
}
