use std::future::Future;
use std::time::Duration;
use nusb::{Device, Interface};
use nusb::transfer::{Completion, Queue, RequestBuffer, ResponseBuffer, TransferError};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::mpsc::error::TryRecvError;
use tracing::{error, info, trace, warn};
use wire_weaver_usb_common::{FrameBuilder, FrameReader, FrameSink, FrameSource};

pub enum DeviceManagerCommand {
    Open {
        device_path: (),
    },
    CloseAll,
}

pub enum DeviceManagerEvent {
    Opened(AsyncDeviceHandle),
    DeviceConnected,
    DeviceDisconnected,
    // Closed
}

pub struct AsyncDeviceHandle {
    pub device_path: (),
    pub commands_tx: Sender<Command>,
    events_rx: Receiver<Event>
}

impl AsyncDeviceHandle {
    pub fn try_recv(&mut self) -> Option<Event> {
        match self.events_rx.try_recv() {
            Ok(ev) => Some(ev),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                warn!("channel closed (dev handle try_recv)");
                None
            }
        }
    }
}

pub enum Command {
    // Open,
    Close,
    Send(Vec<u8>),
    RecycleBuffer(Vec<u8>),
    TestLink,
}

pub enum Event {
    // Connected,
    Disconnected,
    // Opened,
    // Closed,
    Received(Vec<u8>),
    RecycleBuffer(Vec<u8>),
}

pub struct UsbDeviceManager {
    dm_commands_tx: Sender<DeviceManagerCommand>,
    dm_events_rx: Receiver<DeviceManagerEvent>,
}

impl UsbDeviceManager {
    pub fn new() -> Self {
        let (dm_commands_tx, dm_commands_rx) = channel(4);
        let (dm_events_tx, dm_events_rx) = channel(4);
        // let ctx = Context::new().unwrap();
        tokio::spawn(async move {
            dm_worker(dm_commands_rx, dm_events_tx).await;
        });
        Self {
            dm_commands_tx,
            dm_events_rx
        }
    }

    pub fn connect(&mut self) {
        self.dm_commands_tx.try_send(DeviceManagerCommand::Open {device_path: ()}).unwrap();
    }

    pub fn poll(&mut self) -> Option<DeviceManagerEvent> {
        match self.dm_events_rx.try_recv() {
            Ok(ev) => {
                Some(ev)
            }
            Err(TryRecvError::Empty) => {
                None
            }
            Err(_) => {
                warn!("channel closed (dev manager poll)");
                None
            }
        }
    }
}

async fn dm_worker(mut dm_commands_rx: Receiver<DeviceManagerCommand>, dm_events_tx: Sender<DeviceManagerEvent>) {
    loop {
        let Some(cmd) = dm_commands_rx.recv().await else {
            info!("dm_worker exiting");
            return;
        };
        match cmd {
            DeviceManagerCommand::Open { .. } => {
                trace!("Opening device");
                let mut di = nusb::list_devices().unwrap().find(|d| d.vendor_id() == 0xc0de && d.product_id() == 0xcafe).expect("device should be connected");
                info!("Found device: {:?}", di);
                let dev = di.open().unwrap();
                let interface = dev.claim_interface(1).unwrap();
                interface.set_alt_setting(0).unwrap();
                trace!("opened device");

                let (commands_tx, commands_rx) = channel(16);
                let (events_tx, events_rx) = channel(16);
                tokio::spawn(async move {
                    worker(interface, commands_rx, events_tx).await;
                });
                dm_events_tx.send(DeviceManagerEvent::Opened(AsyncDeviceHandle {
                    device_path: (),
                    commands_tx,
                    events_rx
                })).await.unwrap();
            }
            DeviceManagerCommand::CloseAll => {}
        }
    }
}

struct Sink {
    interface: Interface,
    // vec to reuse
}

impl FrameSink for Sink {
    type Error = TransferError;

    async fn write_frame(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        // Try Queue out
        let completion = self.interface.interrupt_out(0x01, data.to_vec()).await;
        match completion.status {
            Ok(_) => {
                trace!("Out ok");
                Ok(())
            }
            Err(e) => {
                error!("Out error: {:?}", e);
                Err(e)
            }
        }
        // let reuse_vec = completion.data.reuse();
    }

    async fn wait_connection(&mut self) {

    }
}

struct Source {
    interface: Interface,
    // vec to reuse
}

impl FrameSource for Source {
    type Error = TransferError;

    async fn read_frame(&mut self, data: &mut [u8]) -> Result<usize, Self::Error> {
        // reuse vec
        // try Queue in
        let completion = self.interface.interrupt_in(0x82, RequestBuffer::new(512)).await;
        match completion.status {
            Ok(_) => {
                info!("read frame {:02x?}", completion.data);
                data[..completion.data.len()].copy_from_slice(&completion.data);
                Ok(completion.data.len())
            }
            Err(e) => {
                error!("In error: {:?}", e);
                Err(e)
            }
        }
    }

    async fn wait_connection(&mut self) {

    }
}

async fn worker(
    interface: Interface,
    mut rx_commands: Receiver<Command>,
    tx_events: Sender<Event>,
) {
    // let (irq_rx_transfer_tx, irq_rx_transfer_rx) = channel(1);
    // let (irq_rx_result_tx, mut irq_rx_result_rx) = channel(1);

    // let (rx_worker_shutdown_tx, rx_worker_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    // let rx_join = tokio::spawn(async move {
    //     rx_worker(irq_rx_transfer_rx, irq_rx_result_tx).await;
    // });

    // let buf = vec![0u8; 512].into_boxed_slice();
    // let timeout = Duration::from_secs(1);
    // let irq_rx_transfer = read_interrupt_async(&device, 0x82, buf, timeout).unwrap();
    // irq_rx_transfer_tx.send(irq_rx_transfer).await.unwrap();

    // tx_assembly_buf, last_sent_instant, max_jitter
    // let mut tx_assembly_buf = [0u8; 512];
    // let mut packet_builder: Option<FrameBuilder> = None;
    let mut builder_buf = [0u8; 1024];
    let mut frame_builder = FrameBuilder::new(&mut builder_buf, Sink { interface: interface.clone() });


    tokio::spawn(async move {
        let mut reader_staging = [0u8; 1024];
        let mut reader_rx = [0u8; 1024];
        let mut frame_reader = FrameReader::new(Source { interface }, &mut reader_staging);
        loop {
            match frame_reader.read_packet(&mut reader_rx).await {
                Ok(len) => {
                    let packet = &reader_rx[..len];
                    info!("Packet: {packet:02x?}");
                }
                Err(e) => {
                    error!("Error {e:?}");
                }
            }

        }
    });

    loop {
        tokio::select! {
            // irq_rx_result = irq_rx_result_rx.recv() => {
            //     let buf = match irq_rx_result {
            //         Some(Ok((buf, bytes_read))) => {
            //             trace!("irq rx transfer success: {:02x?}", &buf[..bytes_read]);
            //             tx_events.send(Event::Received(Vec::from(&buf[..bytes_read]))).await.unwrap();
            //             buf
            //         }
            //         Some(Err((buf, e))) => {
            //             trace!("irq rx transfer error: {e:?}");
            //             buf
            //         }
            //         None => {
            //             info!("Device worker exiting");
            //             break;
            //         }
            //     };
            //
            //     // TODO: Try using more than 1 read transfer in parallel?
            //     let irq_rx_transfer = read_interrupt_async(&device, 0x82, buf, timeout).unwrap();
            //     irq_rx_transfer_tx.send(irq_rx_transfer).await.unwrap();
            // }
            // rx = frame_reader.read_packet()
            cmd = rx_commands.recv() => {
                match cmd {
                    Some(Command::Send(buf)) => {
                        info!("Sending data {buf:?}");
                        frame_builder.write_packet(&buf).await;
                        frame_builder.force_send().await; // TODO: force send on timer
                    }
                    Some(Command::TestLink) => {
                        // let mut packet_builder = FrameBuilder::new(&mut tx_assembly_buf);
                    }
                    Some(Command::RecycleBuffer(buf)) => {

                    }
                    Some(Command::Close) => {
                        info!("Device worker exiting (cmd)");
                        tx_events.send(Event::Disconnected).await.unwrap();
                        break;
                    }
                    None => {
                        info!("Device worker exiting (cmd channel closed)");
                        let r = tx_events.send(Event::Disconnected).await;
                        if r.is_err() {
                            warn!("Channel closed");
                        }
                        break;
                    }
                }
            }
        }
    }

    // Seems to be a bug in rusb async, when device is dropped, outstanding transfers are not seeing that and continue to be awaited
    // rx_worker_shutdown_tx.send(()).unwrap();
    // drop(irq_rx_transfer_tx);
    // _ = rx_join.await;
    info!("Device worker actually returning");
}