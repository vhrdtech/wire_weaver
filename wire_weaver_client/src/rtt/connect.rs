use anyhow::bail;
use probe_rs::probe::{DebugProbeInfo, list::Lister};
use tokio::sync::mpsc;

use super::event_loop::RttHandle;
use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::command::Command;
use crate::event_loop::transport::Selected;

/// Probe listing is quick and synchronous, used from both async and blocking connect.
pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    let Some((target, speed_hz)) = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::Rtt {
            target, speed_hz, ..
        } => Some((target.clone(), *speed_hz)),
        _ => None,
    }) else {
        bail!("Internal error: RTT selected without a target");
    };
    let control_block = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::RttControlBlock(cb) => Some(cb.clone()),
        _ => None,
    });
    let (probe, info) = match select_matching(c, Lister::new().list_all())? {
        Ok(matched) => matched,
        Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
    };
    let Some(cmd_rx) = cmd_rx.take() else {
        bail!("Internal error: cmd_rx is None");
    };
    tokio::spawn(async move {
        super::event_loop::rtt_worker(cmd_rx).await;
    });
    Ok(Selected::Device {
        handle: Box::new(RttHandle {
            probe,
            target,
            speed_hz,
            control_block,
        }),
        info: Box::new(info),
    })
}

/// Returns the only matching probe, or all probes if none did.
///
/// Only VID:PID and serial number filters after the RTT piece are applied, to the probe itself. The ones before it
/// describe the device behind the probe (e.g., a config made by its driver crate, with the device's USB VID:PID),
/// which is not known before connecting. Device API is checked during link setup anyway.
#[allow(clippy::type_complexity)]
fn select_matching(
    c: &ValidatedConfig,
    probes: Vec<DebugProbeInfo>,
) -> Result<Result<(DebugProbeInfo, crate::DeviceInfo), Vec<crate::DeviceInfo>>, crate::Error> {
    let after_rtt = c
        .pieces
        .iter()
        .rposition(|p| matches!(p, ConfigPiece::Rtt { .. }))
        .map_or(0, |i| i + 1);
    let probe_filters = &c.pieces[after_rtt..];
    // same semantics as DeviceInfo::is_matching: alternatives within a kind, all kinds required
    let vid_pid: Vec<(u16, u16)> = probe_filters
        .iter()
        .filter_map(|p| match p {
            ConfigPiece::UsbVidPid { vid, pid } => Some((*vid, *pid)),
            _ => None,
        })
        .collect();
    let serial: Vec<ConfigPiece> = probe_filters
        .iter()
        .filter(|p| {
            matches!(
                p,
                ConfigPiece::SerialEq { .. } | ConfigPiece::SerialContains { .. }
            )
        })
        .cloned()
        .collect();
    let mut matching = vec![];
    let mut unmatched = vec![];
    for probe in probes {
        let info = crate::DeviceInfo::from(&probe);
        let vid_pid_ok =
            vid_pid.is_empty() || vid_pid.contains(&(probe.vendor_id, probe.product_id));
        if vid_pid_ok && info.is_matching(&serial) {
            matching.push((probe, info));
        } else {
            unmatched.push(info);
        }
    }
    if let Some(matched) = matching.pop() {
        if matching.is_empty() {
            Ok(Ok(matched))
        } else {
            let mut devices = vec![matched.1];
            devices.extend(matching.drain(..).map(|(_, info)| info));
            Err(crate::Error::AmbiguousDeviceChoice(devices))
        }
    } else {
        Ok(Err(unmatched))
    }
}
