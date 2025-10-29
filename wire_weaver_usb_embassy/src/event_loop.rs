use crate::init::UsbContext;
use defmt::{error, info, trace, warn};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::driver::{Driver, EndpointError};
use wire_weaver::WireWeaverAsyncApiBackend;
use wire_weaver_usb_link::{
    DisconnectReason, Error as LinkError, Error, MessageKind, ProtocolInfo, WireWeaverUsbLink,
};

pub struct UsbBuffers<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> {
    /// Used to receive USB packets
    rx: [u8; MAX_USB_PACKET_LEN],
    /// Used to assemble frames from multiple USB packets
    rx_message: [u8; MAX_MESSAGE_LEN],
    /// Used to prepare USB packets for transmission
    tx: [u8; MAX_USB_PACKET_LEN],
}

pub struct UsbTimings {
    /// USB packet is not send immediately to avoid sending a lot of small packets
    packet_accumulation_time: Duration,
    /// Used to determine if host driver is stopped and no longer receiving data
    packet_send_timeout: Duration,
    /// How often to send Ping packets
    ww_ping_period: Duration,
}

impl<const MAX_USB_PACKET_LEN: usize, const MAX_MESSAGE_LEN: usize> Default
    for UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>
{
    fn default() -> Self {
        Self {
            rx: [0u8; MAX_USB_PACKET_LEN],
            rx_message: [0u8; MAX_MESSAGE_LEN],
            tx: [0u8; MAX_USB_PACKET_LEN],
        }
    }
}

impl UsbTimings {
    pub fn default_fs() -> Self {
        Self {
            packet_accumulation_time: Duration::from_millis(1),
            packet_send_timeout: Duration::from_millis(100),
            ww_ping_period: Duration::from_millis(3000),
        }
    }

    pub fn default_hs() -> Self {
        Self {
            packet_accumulation_time: Duration::from_micros(125),
            packet_send_timeout: Duration::from_millis(100),
            ww_ping_period: Duration::from_millis(3000),
        }
    }
}

/// Process USB events, wait for connection from wire_weaver_usb_host and forward requests to the provided backend.
/// Call this function in an embassy task.
///
/// This is a convenient way to run an event loop that does only one thing. If you need more advanced setup, you can use the same
/// code in your crate directly and adjust it accordingly.
pub async fn usb_event_loop<
    'd,
    const MAX_USB_PACKET_LEN: usize,
    const MAX_MESSAGE_LEN: usize,
    D: Driver<'d>,
>(
    mut usb_cx: UsbContext<'d, D>,
    buffers: &'d mut UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>,
    backend: &mut impl WireWeaverAsyncApiBackend,
    timings: UsbTimings,
) {
    // Run the USB device.
    let usb_fut = usb_cx.usb.run();
    info!("USB class starting...");

    let user_protocol = ProtocolInfo {
        protocol_id: 13,
        major_version: 0,
        minor_version: 1,
    };
    let client_server_protocol = ProtocolInfo {
        protocol_id: 1,
        major_version: 0,
        minor_version: 1,
    };
    let (tx, rx) = usb_cx.ww.split(); // TODO: do not split?
    let mut link = WireWeaverUsbLink::new(
        client_server_protocol,
        user_protocol,
        tx,
        &mut buffers.tx,
        rx,
        &mut buffers.rx,
    );

    let frame_rx_fut = async {
        loop {
            info!("Waiting for USB cable connection...");
            link.wait_usb_connection().await;
            info!("USB cable is connected");
            // if host app crashed without sending Disconnect, and then incompatible app tried to send
            // data, this will ensure we ignore it before proper version checks happen
            link.silent_disconnect();
            match api_loop::<MAX_MESSAGE_LEN, D>(
                backend,
                &mut link,
                &mut buffers.rx_message,
                &timings,
            )
            .await
            {
                Ok(_) | Err(Error::Disconnected) => info!("api_usb_loop exited on Disconnect"),
                Err(e) => {
                    let r = link
                        .send_disconnect(DisconnectReason::ApplicationCrash)
                        .await;
                    error!("api_loop exited {}, send_disconnect: {}", e, r);
                }
            }
        }
    };

    // Run everything concurrently.
    embassy_futures::join::join(usb_fut, frame_rx_fut).await;
}

async fn api_loop<'d, const MAX_MESSAGE_LEN: usize, D: Driver<'d>>(
    backend: &mut impl WireWeaverAsyncApiBackend,
    link: &mut WireWeaverUsbLink<'d, super::Sender<'d, D>, super::Receiver<'d, D>>,
    rx_message_buf: &mut [u8],
    timings: &UsbTimings,
    // can_frame_rx: &mut Receiver<'static, CriticalSectionRawMutex, SerializedCanFrame>,
) -> Result<(), LinkError<EndpointError, EndpointError>> {
    info!("waiting for link setup...");
    link.wait_link_connection(rx_message_buf).await?;
    info!(
        "link setup done, remote protocol: {}, remote max message size: {}",
        link.remote_protocol(),
        link.remote_max_message_size()
    );
    // can_frame_rx.clear();
    let mut scratch_args = [0u8; MAX_MESSAGE_LEN];
    let mut scratch_event = [0u8; MAX_MESSAGE_LEN];
    let mut scratch_err = [0u8; 32];

    let mut packet_started_instant: Option<Instant> = None;
    loop {
        let delay = if let Some(instant) = packet_started_instant {
            let dt_since_packet_start = Instant::now() - instant;
            timings
                .packet_accumulation_time
                .checked_sub(dt_since_packet_start)
                .unwrap_or(Duration::from_ticks(0))
        } else {
            timings.ww_ping_period
        };
        let tim = Timer::after(delay);
        let message_rx = link.receive_message(rx_message_buf);
        // let can_frame = can_frame_rx.receive();
        match select(tim, message_rx).await {
            Either::First(_) => {
                // timer timeout
                let send_timeout = Timer::after(timings.packet_send_timeout);
                if packet_started_instant.is_some() {
                    packet_started_instant = None;
                    trace!("sending accumulated packet");
                    match select(link.force_send(), send_timeout).await {
                        Either::First(r) => r?,
                        Either::Second(_) => {
                            warn!(
                                "Timeout while force_send'ing, host didn't sent Disconnect?, exiting"
                            );
                            return Ok(());
                        }
                    }
                } else {
                    trace!("sending ping");
                    match select(link.send_ping(), send_timeout).await {
                        Either::First(r) => r?,
                        Either::Second(_) => {
                            warn!(
                                "Timeout while sending ping, host didn't sent Disconnect?, exiting"
                            );
                            return Ok(());
                        }
                    }
                }
            }
            Either::Second(message) => match message? {
                // message from host
                MessageKind::Data(len) => {
                    let message = &rx_message_buf[..len];
                    trace!("message: {:x}", message);
                    match backend
                        .process_bytes(
                            message,
                            &mut scratch_args,
                            &mut scratch_event,
                            &mut scratch_err,
                        )
                        .await
                    {
                        Ok(event_bytes) => {
                            if event_bytes.is_empty() {
                                continue;
                            }
                            let send_msg = link.send_message(event_bytes);
                            let send_timeout = Timer::after(timings.packet_send_timeout);
                            match select(send_msg, send_timeout).await {
                                Either::First(r) => r?,
                                Either::Second(_) => {
                                    warn!(
                                        "Timeout while sending message(ww response), host didn't sent Disconnect?, exiting"
                                    );
                                    return Ok(());
                                }
                            }
                            if link.is_tx_queue_empty() {
                                packet_started_instant = None;
                            } else if packet_started_instant.is_none() {
                                packet_started_instant = Some(Instant::now());
                            }
                        }
                        Err(_e) => {
                            // TODO: send error back
                        }
                    }
                }
                MessageKind::Disconnect(reason) => {
                    info!("Received Disconnect({}), exiting", reason);
                    return Ok(());
                }
                MessageKind::Ping => {
                    // Ignoring Ping from host due to how send is implemented:
                    // Our ping send above will get stuck and timeout, indicating host is disconnected.
                    trace!("Ping");
                }
                _ /* MessageKind::LinkSetup { .. } */ => {} // not used at this stage
            },
        }
    }
}
