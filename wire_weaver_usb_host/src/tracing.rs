use iceoryx2::port::publisher::Publisher;
use iceoryx2::prelude::ZeroCopySend;
use iceoryx2::service::ipc_threadsafe::Service;
use iceoryx2_bb_container::vector::{StaticVec, Vector};
use wire_weaver_usb_link::{PacketSink, PacketSource};

#[derive(ZeroCopySend, Debug)]
#[repr(C)]
pub struct UsbPacket {
    pub ep: u8,
    pub data: StaticVec<u8, 1024>,
}

pub(crate) struct SinkTrace<B> {
    publisher: Publisher<Service, UsbPacket, ()>,
    inner: B,
}

pub(crate) struct SourceTrace<B> {
    publisher: Publisher<Service, UsbPacket, ()>,
    inner: B,
}

impl<B: PacketSink + Send + Sync> SinkTrace<B> {
    pub(crate) fn new(publisher: Publisher<Service, UsbPacket, ()>, inner: B) -> Self {
        Self { publisher, inner }
    }
}

impl<B: PacketSource + Send + Sync> SourceTrace<B> {
    pub(crate) fn new(publisher: Publisher<Service, UsbPacket, ()>, inner: B) -> Self {
        Self { publisher, inner }
    }
}

impl<B: PacketSink> PacketSink for SinkTrace<B> {
    type Error = B::Error;

    async fn write_packet(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let packet = self.publisher.loan_uninit().unwrap();
        let mut data_trace = StaticVec::new();
        data_trace.resize(data.len(), 0).unwrap();
        data_trace[..data.len()].copy_from_slice(data);
        let packet = packet.write_payload(UsbPacket {
            ep: 0,
            data: data_trace,
        });
        packet.send().unwrap();

        self.inner.write_packet(data).await
    }
}

impl<B: PacketSource> PacketSource for SourceTrace<B> {
    type Error = B::Error;

    async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
        let len = self.inner.read_packet(data).await?;
        let packet = self.publisher.loan_uninit().unwrap();
        let mut data_trace = StaticVec::new();
        data_trace.resize(len, 0).unwrap();
        data_trace[..len].copy_from_slice(&data[..len]);
        let packet = packet.write_payload(UsbPacket {
            ep: 0,
            data: data_trace,
        });
        packet.send().unwrap();

        Ok(len)
    }
}
