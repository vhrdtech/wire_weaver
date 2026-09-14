//! Async USB wrapper around the sans-IO [Core]: owns the command channel, nusb endpoints,
//! the framer and timers, and only shuffles [Input]s in and [Output]s out.

use std::time::{Duration, Instant};

use nusb::DeviceInfo;
use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::tracing::Tracer;
use super::ww_nusb::{ERR_WRITE_PACKET_TIMEOUT, Sink, Source};
use crate::event_loop::command::Command;
use crate::event_loop::core::{Core, Input, MAX_MESSAGE_SIZE, Output};
use crate::event_loop::framing::Framing;

/// USB packets are checked by the hardware, but a CRC over each split message still catches
/// packets lost or reordered on re-connection.
type UsbFraming = Framing<ww_link::Head, ww_link::Checksum, ww_link::Tail>;

/// nusb endpoints and framer, present only while connected.
struct Transport {
    sink: Sink,
    source: Source,
    framing: UsbFraming,
    tracer: Tracer,
    _device: nusb::Device,
}

pub async fn usb_worker(mut cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    let mut core = Core::new();
    let mut transport: Option<Transport> = None;
    let mut rx_buf = vec![0u8; 1024];
    let mut frames: Vec<Vec<u8>> = vec![];

    let result = loop {
        // Drain outputs first, do not feed new inputs until done
        let mut exit = None;
        while let Some(output) = core.poll_output() {
            // match is split from the await below, so that non-Send `handle` is not held across it
            match output {
                Output::Connect(handle) => {
                    match connect(handle) {
                        Ok(t) => {
                            transport = Some(t);
                            core.handle(Instant::now(), Input::TransportUp);
                        }
                        Err(e) => core.handle(Instant::now(), Input::TransportError(e)),
                    }
                    continue;
                }
                Output::Send { kind, payload } => {
                    let Some(t) = transport.as_mut() else {
                        warn!("Send without transport, dropping");
                        continue;
                    };
                    if let Err(e) = t.framing.write(kind, &payload, &mut frames) {
                        // Core is responsible for not sending oversized messages, so this is a bug or a
                        // wrong frame size, not something the device did
                        core.handle(Instant::now(), Input::TransportError(e.to_string()));
                        continue;
                    }
                }
                Output::Flush => {
                    if let Some(t) = transport.as_mut() {
                        t.framing.flush(&mut frames);
                    }
                }
                Output::Exit(result) => {
                    exit = Some(result);
                    break;
                }
            }
            if let Some(t) = transport.as_mut()
                && let Err(e) = send_frames(t, &mut frames).await
            {
                core.handle(Instant::now(), Input::TransportError(e));
            }
        }
        if let Some(result) = exit {
            if let Some(t) = transport.as_mut() {
                t.framing.flush(&mut frames);
                _ = send_frames(t, &mut frames).await;
            }
            // give the last (Disconnect) transfer a chance to actually go out before endpoints are dropped
            tokio::time::sleep(Duration::from_millis(3)).await;
            transport = None;
            break result;
        }

        let deadline = core.poll_timeout();
        let timer = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        let frame = async {
            match transport.as_mut() {
                Some(t) => t.source.read_packet(&mut rx_buf).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(cmd) => core.handle(Instant::now(), Input::Command(cmd)),
                    None => core.handle(Instant::now(), Input::CommanderDropped),
                }
            }
            frame = frame => {
                match frame {
                    Ok(len) => {
                        let frame = &rx_buf[..len];
                        trace!("rx frame: {len}: {frame:02x?}");
                        let now = Instant::now();
                        // transport is Some, otherwise this branch would be pending
                        if let Some(t) = transport.as_mut() {
                            t.tracer.rx(frame);
                            if let Err(e) = t.framing.stage(frame) {
                                warn!("{e}, dropping frame");
                            }
                            while let Some((kind, payload)) = t.framing.next_message() {
                                core.handle(now, Input::Message { kind, payload });
                            }
                        }
                    }
                    Err(e) => core.handle(Instant::now(), Input::TransportError(describe_transfer_error(e))),
                }
            }
            _ = timer => {
                core.handle(Instant::now(), Input::Timer);
            }
        }
    };
    drop(transport);

    let (exited_tx, residual) = core.into_residual(cmd_rx, result);
    if let Some(tx) = exited_tx {
        _ = tx.send(residual);
    }
    debug!("usb worker exited");
}

async fn send_frames(t: &mut Transport, frames: &mut Vec<Vec<u8>>) -> Result<(), String> {
    for frame in frames.drain(..) {
        trace!("tx frame: {}: {frame:02x?}", frame.len());
        t.tracer.tx(&frame);
        t.sink
            .write_packet(&frame)
            .await
            .map_err(describe_transfer_error)?;
    }
    Ok(())
}

fn connect(handle: Box<dyn std::any::Any + Send>) -> Result<Transport, String> {
    let di = handle
        .downcast::<DeviceInfo>()
        .map_err(|_| "expected nusb::DeviceInfo handle".to_string())?;
    let dev = super::connect::connect(&di).map_err(|e| format!("{e:#}"))?;
    trace!("max_packet_size: {}", dev.max_packet_size);
    let is_bulk = dev.transfer_type == TransferType::Bulk;
    let sink =
        Sink::new(&dev.interface, dev.max_packet_size, is_bulk).map_err(|e| e.to_string())?;
    let source =
        Source::new(&dev.interface, dev.max_packet_size, is_bulk).map_err(|e| e.to_string())?;
    let tracer = Tracer::new(&di).map_err(|e| format!("usb tracing: {e:#}"))?;
    Ok(Transport {
        sink,
        source,
        framing: UsbFraming::new(dev.max_packet_size, MAX_MESSAGE_SIZE),
        tracer,
        _device: dev.device,
    })
}

fn describe_transfer_error(e: TransferError) -> String {
    match e {
        // If device boots, but then ends up in an endless loop or in HardFault, USB device is still detected by the host,
        // but no transfers go through.
        TransferError::Unknown(ERR_WRITE_PACKET_TIMEOUT) => {
            "Device is not accepting USB transfers, it might be in an endless loop or in HardFault"
                .into()
        }
        e => format!("USB transfer error: {e:?}"),
    }
}
