use anyhow::bail;
use probe_rs::probe::{DebugProbeInfo, list::Lister};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    config::{ConfigPiece, ValidatedConfig},
    event_loop::DeviceHandle,
    internal::Command,
};

pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Option<DeviceHandle>, anyhow::Error> {
    start_event_loop(c, cmd_rx)
}

fn start_event_loop(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Option<DeviceHandle>, anyhow::Error> {
    let Some(matched) = select_matching(c)? else {
        return Ok(None);
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    // tokio::spawn(async move {
    //     super::event_loop::usb_worker(cmd_rx).await;
    // });
    let handle = Box::new(matched);
    Ok(Some(handle))
}

fn select_matching(c: &ValidatedConfig) -> Result<Option<DebugProbeInfo>, crate::Error> {
    let lister = Lister::new();

    let mut matching = vec![];
    for device in lister.list_all() {
        let info = crate::DeviceInfo::from(&device);
        let vid_pid_match = vid_pid_match(&c.pieces, &device);
        let port_chain_match = port_chain_match(&c.pieces, &device);
        if vid_pid_match || port_chain_match || info.is_matching(c) {
            matching.push((device, info));
        }
    }
    if let Some((device, info)) = matching.pop() {
        if matching.is_empty() {
            Ok(Some(device))
        } else {
            let mut devices = vec![info];
            devices.extend(matching.drain(..).map(|(_, info)| info));
            Err(crate::Error::AmbiguousDeviceChoice(devices))
        }
    } else {
        Ok(None)
    }
}

fn vid_pid_match(pieces: &[ConfigPiece], info: &DebugProbeInfo) -> bool {
    pieces.iter().any(|p| {
        if let ConfigPiece::UsbVidPid { vid, pid } = p {
            info.vendor_id == *vid && info.product_id == *pid
        } else {
            false
        }
    })
}

fn port_chain_match(pieces: &[ConfigPiece], info: &DebugProbeInfo) -> bool {
    pieces.iter().any(|p| {
        if let ConfigPiece::UsbPath { .. } = p {
            // TODO: port chain for RTT adapters
            warn!("Port chain for probes is not implemented yet");
            false
        } else {
            false
        }
    })
}
