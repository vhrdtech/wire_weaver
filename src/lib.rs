use rusb::constants::{LIBUSB_ENDPOINT_DIR_MASK, LIBUSB_ENDPOINT_IN, LIBUSB_ENDPOINT_OUT};
use rusb::{DeviceHandle, UsbContext as _};
use rusb_async::{Context, DeviceHandleExt as _, Transfer};
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::mpsc::error::TryRecvError;
use tracing::{error, info, trace, warn};

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
        let ctx = Context::new().unwrap();
        tokio::spawn(async move {
            dm_worker(ctx, dm_commands_rx, dm_events_tx).await;
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

async fn dm_worker(ctx: Context, mut dm_commands_rx: Receiver<DeviceManagerCommand>, dm_events_tx: Sender<DeviceManagerEvent>) {
    loop {
        let Some(cmd) = dm_commands_rx.recv().await else {
            info!("dm_worker exiting");
            return;
        };
        match cmd {
            DeviceManagerCommand::Open { .. } => {
                trace!("Opening device");
                let mut dev = ctx.open_device_with_vid_pid(0xc0de, 0xcafe).unwrap();
                dev.claim_interface(1).unwrap();
                dev.set_alternate_setting(1, 0).unwrap();
                // dev.read_interrupt_async(0, a, b);

                let (commands_tx, commands_rx) = channel(16);
                let (events_tx, events_rx) = channel(16);
                tokio::spawn(async move {
                    worker(dev, commands_rx, events_tx).await;
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

async fn worker(
    device: DeviceHandle<Context>,
    mut rx_commands: Receiver<Command>,
    tx_events: Sender<Event>,
) {
    let (irq_rx_transfer_tx, irq_rx_transfer_rx) = channel(1);
    let (irq_rx_result_tx, mut irq_rx_result_rx) = channel(1);

    // let (rx_worker_shutdown_tx, rx_worker_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let rx_join = tokio::spawn(async move {
        rx_worker(irq_rx_transfer_rx, irq_rx_result_tx).await;
    });

    let buf = vec![0u8; 512].into_boxed_slice();
    let timeout = Duration::from_secs(1);
    let irq_rx_transfer = read_interrupt_async(&device, 0x82, buf, timeout).unwrap();
    irq_rx_transfer_tx.send(irq_rx_transfer).await.unwrap();

    loop {
        tokio::select! {
            irq_rx_result = irq_rx_result_rx.recv() => {
                let buf = match irq_rx_result {
                    Some(Ok((buf, bytes_read))) => {
                        trace!("irq rx transfer success: {:02x?}", &buf[..bytes_read]);
                        tx_events.send(Event::Received(Vec::from(&buf[..bytes_read]))).await.unwrap();
                        buf
                    }
                    Some(Err((buf, e))) => {
                        trace!("irq rx transfer error: {e:?}");
                        buf
                    }
                    None => {
                        info!("Device worker exiting");
                        break;
                    }
                };

                // TODO: Try using more than 1 read transfer in parallel?
                let irq_rx_transfer = read_interrupt_async(&device, 0x82, buf, timeout).unwrap();
                irq_rx_transfer_tx.send(irq_rx_transfer).await.unwrap();
            }
            cmd = rx_commands.recv() => {
                match cmd {
                    Some(Command::Send(buf)) => {
                        let tx_transfer = write_interrupt_async(&device, 1, buf.into(), timeout).unwrap();
                        tokio::spawn(async move {
                            match tx_transfer.await {
                                Ok((buf, written)) => {
                                    trace!("tx ok: {written}B {:02x?}", buf);
                                }
                                Err((buf, e)) => {
                                    error!("tx err: {e:?}");
                                }
                            }
                            // TODO: Recycle buf
                        });
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
    drop(irq_rx_transfer_tx);
    _ = rx_join.await;
    info!("Device worker actually returning");
}

type TransferResult = Result<(Box<[u8]>, usize), (Box<[u8]>, rusb::Error)>;

async fn rx_worker(
    mut transfer_rx: Receiver<Transfer<Context>>,
    result_tx: Sender<TransferResult>,
) {
    loop {
        let Some(transfer) = transfer_rx.recv().await else {
            trace!("rx_worker exiting (channel closed)");
            return;
        };
        // trace!("got rx transfer, awaiting");
        let result = transfer.await;
        // trace!("rx transfer awaited");
        if let Err(_) = result_tx.send(result).await {
            error!("rx_worker send result failed");
        }
    }
}

fn read_interrupt_async(
    device: &DeviceHandle<Context>,
    endpoint: u8,
    data: Box<[u8]>,
    timeout: Duration,
) -> Result<Transfer<Context>, (Box<[u8]>, rusb::Error)> {
    if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_IN {
        return Err((data, rusb::Error::InvalidParam));
    }

    let transfer = Transfer::new_interrupt_transfer(device, endpoint, data, timeout)?;
    Ok(transfer)
}

fn write_interrupt_async(
    device: &DeviceHandle<Context>,
    endpoint: u8,
    data: Box<[u8]>,
    timeout: Duration,
) -> Result<Transfer<Context>, (Box<[u8]>, rusb::Error)> {
    if endpoint & LIBUSB_ENDPOINT_DIR_MASK != LIBUSB_ENDPOINT_OUT {
        return Err((data, rusb::Error::InvalidParam));
    }

    let transfer = Transfer::new_interrupt_transfer(device, endpoint, data, timeout)?;

    Ok(transfer)
}
