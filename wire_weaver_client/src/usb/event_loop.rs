//! Async USB wrapper around the sans-IO [Core]: owns the command channel, nusb endpoints
//! and timers, and only shuffles [Input]s in and [Output]s out.

use std::time::{Duration, Instant};

use nusb::DeviceInfo;
use nusb::descriptors::TransferType;
use nusb::transfer::TransferError;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::tracing::Tracer;
use super::ww_nusb::{ERR_WRITE_PACKET_TIMEOUT, Sink, Source};
use crate::event_loop::command::Command;
use crate::event_loop::core::{Core, Input, Output};

/// nusb endpoints, present only while connected.
struct Transport {
    sink: Sink,
    source: Source,
    max_packet_size: usize,
    tracer: Tracer,
    _device: nusb::Device,
}

pub async fn usb_worker(mut cmd_rx: mpsc::Receiver<Command>) {
    debug!("usb worker started");
    let mut core = Core::new();
    let mut transport: Option<Transport> = None;
    let mut rx_buf = vec![0u8; 1024];

    let result = loop {
        // Drain outputs first, do not feed new inputs until done
        let mut exit = None;
        while let Some(output) = core.poll_output() {
            // match is split from the await below, so that non-Send `handle` is not held across it
            let frame = match output {
                Output::Connect(handle) => {
                    match connect(handle) {
                        Ok(t) => {
                            let frame_size = t.max_packet_size;
                            transport = Some(t);
                            core.handle(Instant::now(), Input::TransportUp { frame_size });
                        }
                        Err(e) => core.handle(Instant::now(), Input::TransportError(e)),
                    }
                    continue;
                }
                Output::SendFrame(frame) => frame,
                Output::Exit(result) => {
                    exit = Some(result);
                    break;
                }
            };
            let Some(t) = transport.as_mut() else {
                warn!("SendFrame without transport, dropping");
                continue;
            };
            t.tracer.tx(&frame);
            if let Err(e) = t.sink.write_packet(&frame).await {
                let e = describe_transfer_error(e);
                core.handle(Instant::now(), Input::TransportError(e));
            }
        }
        if let Some(result) = exit {
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
                        if let Some(t) = &transport {
                            t.tracer.rx(&rx_buf[..len]);
                        }
                        core.handle(Instant::now(), Input::Frame(&rx_buf[..len]))
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
        max_packet_size: dev.max_packet_size,
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
