use std::thread::sleep;
use std::time::Duration;
use nusb::{Interface};
use nusb::transfer::{RequestBuffer, TransferError};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::mpsc::error::TryRecvError;
use tracing::{error, info, trace, warn};
use wire_weaver_usb_link::{PacketSender, PacketReceiver, FrameSink, FrameSource, LinkMgmtCmd, ProtocolInfo, PacketKind, ReceiveError};

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
                let di = nusb::list_devices().unwrap().find(|d| d.vendor_id() == 0xc0de && d.product_id() == 0xcafe).expect("device should be connected");
                info!("Found device: {:?}", di);
                let dev = di.open().unwrap();
                let interface = dev.claim_interface(0).unwrap();
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
                trace!("wrote: {:02x?}", data);
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

    async fn rx_from_source(&mut self) -> LinkMgmtCmd {
        unreachable!()
    }

    fn try_rx_from_source(&mut self) -> Option<LinkMgmtCmd> {
        None
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
        let completion = self.interface.interrupt_in(0x81, RequestBuffer::new(512)).await;
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

    fn send_to_sink(&mut self, _msg: LinkMgmtCmd) {

    }
}

async fn worker(
    interface: Interface,
    mut rx_commands: Receiver<Command>,
    tx_events: Sender<Event>,
) {
    let user_protocol = ProtocolInfo {
        protocol_id: 7,
        major_version: 0,
        minor_version: 1,
    };
    let mut builder_buf = [0u8; 1024];
    let mut packet_sender = PacketSender::new(&mut builder_buf, Sink { interface: interface.clone() }, user_protocol);
    const MAX_PACKET_SIZE: usize = 2048;
    // for some reason first packet seem to be lost when connecting for the second tmie, so send nop
    packet_sender.send_nop().await.unwrap();
    packet_sender.send_link_setup(MAX_PACKET_SIZE as u32).await.unwrap();

    tokio::spawn(async move {
        let mut reader_staging = [0u8; 512]; // max USB transfer size, TODO: use 1024 USB transfers?
        let mut reader_rx = [0u8; MAX_PACKET_SIZE];
        let mut packet_receiver = PacketReceiver::new(Source { interface }, &mut reader_staging, user_protocol);
        loop {
            match packet_receiver.receive_packet(&mut reader_rx).await {
                Ok(PacketKind::Data(len)) => {
                    let packet = &reader_rx[..len];
                    info!("Packet: {packet:02x?}");
                }
                Ok(PacketKind::Disconnect) => {
                    info!("Received Disconnect, exiting");
                    break;
                }
                Ok(PacketKind::Ping) => {
                    trace!("Ping");
                }
                Ok(PacketKind::LinkInfo { remote_max_packet_size, remote_user_protocol }) => {
                    info!("Link info: protocol {remote_user_protocol:?} max packet size: {remote_max_packet_size}");
                }
                Err(ReceiveError::EmptyFrame) => {
                    info!("Receive empty frame");
                    // break;
                }
                Err(ReceiveError::ProtocolsVersionsMismatch) => {
                    warn!("Protocols versions mismatch");
                }
                Err(ReceiveError::SourceError(e)) => {
                    error!("Transfer error: {e:?}, exiting");
                    break;
                }
                Err(ReceiveError::InternalBufOverflow) => {
                    warn!("Internal buf overflow while receiving packet");
                }
            }

        }
    });

    loop {
        tokio::select! {
            cmd = rx_commands.recv() => {
                match cmd {
                    Some(Command::Send(buf)) => {
                        info!("Sending data {buf:?}");
                        packet_sender.send_packet(&buf).await;
                        packet_sender.force_send().await; // TODO: force send on timer
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

    info!("Device worker actually returning");
}