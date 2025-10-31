use crate::UsbServer;
use defmt::{error, info, trace};
use embassy_futures::select::{Either, Either3, select3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Receiver;
use embassy_time::{Duration, Instant, Timer};
use embassy_usb::driver::{Driver, EndpointError};
use wire_weaver::WireWeaverAsyncApiBackend;
use wire_weaver_usb_link::{DisconnectReason, Error as LinkError, MessageKind, WireWeaverUsbLink};

// TODO: tune ignore timer duration
const IGNORE_TIMER_DURATION: Duration = Duration::from_micros(10);

pub struct UsbTimings {
    /// USB packet is not send immediately to avoid sending a lot of small packets
    packet_accumulation_time: Duration,
    /// Used to determine if host driver is stopped and no longer receiving data
    pub(crate) packet_send_timeout: Duration,
    /// How often to send Ping packets
    ww_ping_period: Duration,
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

impl<'d, D: Driver<'d>, B: WireWeaverAsyncApiBackend> UsbServer<'d, D, B> {
    pub async fn run(&mut self) -> ! {
        let usb_fut = self.usb.run();
        let req_fut = async {
            loop {
                info!("Waiting for USB cable connection...");
                self.link.wait_usb_connection().await;
                info!("USB cable is connected");
                // if host app crashed without sending Disconnect, and then incompatible app tried to send
                // data, this will ensure we ignore it before proper version checks happen
                self.link.silent_disconnect();
                match api_loop(
                    &mut self.state,
                    &mut self.link,
                    &mut self.call_publish_rx,
                    self.rx_message,
                    self.scratch_args,
                    self.scratch_event,
                    &self.timings,
                )
                .await
                {
                    Ok(_) | Err(LinkError::Disconnected) => {
                        info!("api_usb_loop exited on Disconnect")
                    }
                    Err(e) => {
                        let r = self
                            .link
                            .send_disconnect(DisconnectReason::ApplicationCrash)
                            .await;
                        error!("api_loop exited {}, send_disconnect: {}", e, r);
                    }
                }
            }
        };
        embassy_futures::join::join(usb_fut, req_fut).await.0
    }
}

// NOTE: when host closes USB device, our attempts to send a packet will get stuck and timeout in super::Sender
async fn api_loop<'d, D: Driver<'d>>(
    backend: &mut impl WireWeaverAsyncApiBackend,
    link: &mut WireWeaverUsbLink<'d, super::Sender<'d, D>, super::Receiver<'d, D>>,
    call_publish_rx: &mut Receiver<'d, CriticalSectionRawMutex, (), 1>,
    rx_message_buf: &mut [u8],
    scratch_args: &mut [u8],
    scratch_event: &mut [u8],
    timings: &UsbTimings,
) -> Result<(), LinkError<EndpointError, EndpointError>> {
    info!("waiting for link setup...");
    link.wait_link_connection(rx_message_buf).await?;
    info!(
        "link setup done, remote protocol: {}, remote max message size: {}",
        link.remote_protocol(),
        link.remote_max_message_size()
    );

    let mut scratch_err = [0u8; 32];
    let mut packet_started_instant: Option<Instant> = None;
    let mut next_ping_instant = Instant::now() + timings.ww_ping_period;
    loop {
        let till_force_send = if let Some(instant) = packet_started_instant {
            let dt_since_packet_start = Instant::now()
                .checked_duration_since(instant)
                .unwrap_or(Duration::from_ticks(0));
            let till_force_send = timings
                .packet_accumulation_time
                .checked_sub(dt_since_packet_start)
                .unwrap_or(Duration::from_ticks(0));
            if till_force_send < IGNORE_TIMER_DURATION {
                packet_started_instant = None;
                trace!("sending accumulated packet");
                link.force_send().await?;
                next_ping_instant = Instant::now() + timings.ww_ping_period;
                None
            } else {
                Some(till_force_send)
            }
        } else {
            None
        };
        let till_ping = next_ping_instant
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::from_ticks(0));
        let till_ping = if till_ping < IGNORE_TIMER_DURATION {
            trace!("sending ping");
            link.send_ping().await?;
            next_ping_instant = Instant::now() + timings.ww_ping_period;
            timings.ww_ping_period
        } else {
            till_ping
        };
        let till_min = till_force_send
            .map(|f| f.min(till_ping))
            .unwrap_or(till_ping);
        let tim = Timer::after(till_min);

        let message_rx = link.receive_message(rx_message_buf);
        match select3(tim, message_rx, call_publish_rx.receive()).await {
            Either3::First(_) => {
                // timer timeout
                if packet_started_instant.is_some() {
                    packet_started_instant = None;
                    trace!("sending accumulated packet");
                    link.force_send().await?;
                } else {
                    trace!("sending ping");
                    link.send_ping().await?;
                }
                next_ping_instant = Instant::now() + timings.ww_ping_period;
            }
            Either3::Second(message) => match message? {
                // message from host
                MessageKind::Data(len) => {
                    let message = &rx_message_buf[..len];
                    trace!("message: {:02x}", message);
                    match backend
                        .process_bytes(
                            message,
                            scratch_args,
                            scratch_event,
                            &mut scratch_err,
                        )
                        .await
                    {
                        Ok(event_bytes) => {
                            if event_bytes.is_empty() {
                                continue;
                            }
                            let packets_sent_prev = link.sender_stats().packets_sent;
                           link.send_message(event_bytes).await?;
                            if link.is_tx_queue_empty() {
                                packet_started_instant = None;
                            } else if packet_started_instant.is_none() {
                                packet_started_instant = Some(Instant::now());
                            }
                            if link.sender_stats().packets_sent != packets_sent_prev {
                                // if at least one USB packet was just sent, there is no need to seng ping too soon
                                next_ping_instant = Instant::now() + timings.ww_ping_period;
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
                    trace!("ping from host");
                }
                _ /* MessageKind::LinkSetup { .. } */ => {} // not used at this stage
            },
            Either3::Third(_) => {
                // notification from user code to call send_updates() on the backend
                let packets_sent_prev = link.sender_stats().packets_sent;
                backend
                    .send_updates(link, scratch_args, scratch_event)
                    .await;
                if link.is_tx_queue_empty() {
                    packet_started_instant = None;
                } else if packet_started_instant.is_none() {
                    packet_started_instant = Some(Instant::now());
                }
                if link.sender_stats().packets_sent != packets_sent_prev {
                    // if at least one USB packet was just sent, there is no need to seng ping too soon
                    next_ping_instant = Instant::now() + timings.ww_ping_period;
                }
            }
        }
    }
}
