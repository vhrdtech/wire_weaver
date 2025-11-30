//! Handle packet sending and receiving between nusb and wire_weaver_usb_link
use nusb::Interface;
use nusb::transfer::{RequestBuffer, TransferError};
use tracing::{error, trace};
use wire_weaver_usb_link::{PacketSink, PacketSource};

pub(crate) struct Sink {
    interface: Interface,
    data_reuse: Option<Vec<u8>>,
    _irq_max_packet_size: usize,
    // vec to reuse
}

impl Sink {
    pub fn new(interface: Interface, irq_max_packet_size: usize) -> Self {
        Sink {
            interface,
            data_reuse: Some(Vec::with_capacity(irq_max_packet_size)),
            _irq_max_packet_size: irq_max_packet_size,
        }
    }
}

impl PacketSink for Sink {
    type Error = TransferError;

    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        // TODO: Try Queue out
        let data_owned = match self.data_reuse.take() {
            Some(mut r) => {
                r.resize(data.len(), 0);
                r.copy_from_slice(data);
                r
            }
            None => data.to_vec(),
        };
        let completion = self.interface.interrupt_out(0x01, data_owned).await;
        match completion.status {
            Ok(_) => {
                trace!("irq wrote packet: {:02x?}", data);
                self.data_reuse = Some(completion.data.reuse());
                Ok(())
            }
            Err(e) => {
                error!("irq write error: {:?}", e);
                Err(e)
            }
        }
    }
}

pub(crate) struct Source {
    interface: Interface,
    data_reuse: Option<Vec<u8>>,
    irq_max_packet_size: usize,
    // vec to reuse
}

impl Source {
    pub fn new(interface: Interface, irq_max_packet_size: usize) -> Self {
        Source {
            interface,
            data_reuse: Some(Vec::with_capacity(irq_max_packet_size)),
            irq_max_packet_size,
        }
    }
}

impl PacketSource for Source {
    type Error = TransferError;

    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
        // TODO: Try Queue in
        let request_buf = match self.data_reuse.take() {
            Some(r) => RequestBuffer::reuse(r, self.irq_max_packet_size),
            None => RequestBuffer::new(self.irq_max_packet_size),
        };
        let completion = self.interface.interrupt_in(0x81, request_buf).await;
        match completion.status {
            Ok(_) => {
                trace!("irq read packet: {:02x?}", completion.data);
                data[..completion.data.len()].copy_from_slice(&completion.data);
                let len = completion.data.len();
                self.data_reuse = Some(completion.data);
                Ok(len)
            }
            Err(e) => {
                error!("irq read error: {:?}", e);
                Err(e)
            }
        }
    }
}
