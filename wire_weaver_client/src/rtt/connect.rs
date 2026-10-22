use anyhow::bail;
use probe_rs::probe::{DebugProbeInfo, list::Lister};
use tokio::sync::mpsc;

use super::event_loop::RttHandle;
use crate::config::{ConfigPiece, ValidatedConfig};
use crate::event_loop::command::Command;
use crate::event_loop::transport::Selected;

/// [try_connect] for `connect()`: through the ww hub when the hub holds the probe's device, or its direct holder
/// hands it over (CLT-43; [HubMode](crate::hub::HubMode), without the open counter).
pub(crate) async fn try_connect_routed(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
) -> Result<Selected, anyhow::Error> {
    #[cfg(feature = "hub")]
    {
        let (probe, info) = match select(c)? {
            Ok(matched) => matched,
            Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
        };
        let key = crate::holders::key_for(crate::holders::DeviceRef::Rtt {
            probe_serial: probe.serial_number.as_deref().unwrap_or_default(),
            chip: &rtt_target(c)?.0,
        });
        if let crate::hub::Route::Hub(tunnel) = crate::hub::route(c, &key, false).await? {
            let Some(cmd_rx) = cmd_rx.take() else {
                bail!("Internal error: cmd_rx is None");
            };
            tokio::spawn(crate::local_socket::worker(cmd_rx));
            let mut info = info;
            info.location = format!("{} through the ww hub", info.location);
            return Ok(Selected::Device {
                handle: tunnel,
                info: Box::new(info),
                via_hub: true,
            });
        }
        start(c, probe, info, cmd_rx, true)
    }
    #[cfg(not(feature = "hub"))]
    try_connect(c, cmd_rx, true)
}

fn rtt_target(c: &ValidatedConfig) -> Result<(String, Option<u32>), anyhow::Error> {
    match c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::Rtt {
            target, speed_hz, ..
        } => Some((target.clone(), *speed_hz)),
        _ => None,
    }) {
        Some(t) => Ok(t),
        None => bail!("Internal error: RTT selected without a target"),
    }
}

#[allow(clippy::type_complexity)]
fn select(
    c: &ValidatedConfig,
) -> Result<Result<(DebugProbeInfo, crate::DeviceInfo), Vec<crate::DeviceInfo>>, anyhow::Error> {
    rtt_target(c)?;
    Ok(select_matching(c, Lister::new().list_all())?)
}

/// Probe listing is quick and synchronous, used from both async and blocking connect. `supervised`: the connection
/// reconnects by itself (`connect()`), so it may hand the probe to the hub when asked (CLT-43).
pub(crate) fn try_connect(
    c: &ValidatedConfig,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
    supervised: bool,
) -> Result<Selected, anyhow::Error> {
    let (probe, info) = match select(c)? {
        Ok(matched) => matched,
        Err(unmatched) => return Ok(Selected::NotFound { unmatched }),
    };
    start(c, probe, info, cmd_rx, supervised)
}

fn start(
    c: &ValidatedConfig,
    probe: DebugProbeInfo,
    info: crate::DeviceInfo,
    cmd_rx: &mut Option<mpsc::Receiver<Command>>,
    supervised: bool,
) -> Result<Selected, anyhow::Error> {
    let (target, speed_hz) = rtt_target(c)?;
    let control_block = c.pieces.iter().rev().find_map(|p| match p {
        ConfigPiece::RttControlBlock(cb) => Some(cb.clone()),
        _ => None,
    });
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
            meta: c.client_meta.clone(),
            holder_mode: c.holder_mode,
            handover: c.handover_offer(supervised),
        }),
        info: Box::new(info),
        via_hub: false,
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
