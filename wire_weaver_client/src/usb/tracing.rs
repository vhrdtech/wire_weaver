//! Stream all USB packets over iceoryx2 to a debugger (feature `usb-tracing`).

#[cfg(feature = "usb-tracing")]
mod imp {
    use iceoryx2::port::publisher::Publisher;
    use iceoryx2::prelude::*;
    use iceoryx2::service::ipc_threadsafe::Service;
    use iceoryx2_bb_container::vector::{StaticVec, Vector};

    #[derive(ZeroCopySend, Debug)]
    #[repr(C)]
    pub struct UsbPacket {
        pub ep: u8,
        pub data: StaticVec<u8, 1024>,
    }

    pub struct Tracer {
        _node: Node<Service>,
        tx: Publisher<Service, UsbPacket, ()>,
        rx: Publisher<Service, UsbPacket, ()>,
    }

    impl Tracer {
        pub fn new(di: &nusb::DeviceInfo) -> anyhow::Result<Self> {
            let node = NodeBuilder::new().create::<Service>()?;
            let name = format!("WireWeaver/UsbTrace/{}-{:?}", di.bus_id(), di.port_chain());
            let publisher = |suffix: &str| -> anyhow::Result<_> {
                let service = node
                    .service_builder(&ServiceName::new(format!("{name}/{suffix}").as_str())?)
                    .publish_subscribe::<UsbPacket>()
                    .open_or_create()?;
                Ok(service.publisher_builder().create()?)
            };
            Ok(Tracer {
                tx: publisher("tx")?,
                rx: publisher("rx")?,
                _node: node,
            })
        }

        pub fn tx(&self, frame: &[u8]) {
            Self::publish(&self.tx, frame);
        }

        pub fn rx(&self, frame: &[u8]) {
            Self::publish(&self.rx, frame);
        }

        fn publish(publisher: &Publisher<Service, UsbPacket, ()>, frame: &[u8]) {
            let Ok(packet) = publisher.loan_uninit() else {
                return;
            };
            let mut data = StaticVec::new();
            if data.resize(frame.len(), 0).is_err() {
                return;
            }
            data[..frame.len()].copy_from_slice(frame);
            _ = packet.write_payload(UsbPacket { ep: 0, data }).send();
        }
    }
}

#[cfg(not(feature = "usb-tracing"))]
mod imp {
    pub struct Tracer;

    impl Tracer {
        pub fn new(_di: &nusb::DeviceInfo) -> anyhow::Result<Self> {
            Ok(Tracer)
        }
        pub fn tx(&self, _frame: &[u8]) {}
        pub fn rx(&self, _frame: &[u8]) {}
    }
}

pub(crate) use imp::Tracer;
