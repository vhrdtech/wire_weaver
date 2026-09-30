//! Shell completion candidates for device selection flags, taken from connected devices.
//!
//! Completions are produced by the `ww` binary itself when the shell calls it with `COMPLETE=<shell>` set
//! (see [clap_complete::CompleteEnv]), so values are listed live, without opening the devices.

use clap_complete::CompletionCandidate;
use std::collections::BTreeMap;
use wire_weaver_client::DeviceInfo;

/// Devices reporting a WireWeaver API id, empty on any error (completion must never fail loudly).
fn connected_devices() -> Vec<DeviceInfo> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return vec![];
    };
    rt.block_on(wire_weaver_client::list_usb_devices())
        .unwrap_or_default()
}

/// One candidate per distinct non-empty value, with the product (and label) of the devices it came from as help.
fn candidates(value: impl Fn(&DeviceInfo) -> Vec<String>) -> Vec<CompletionCandidate> {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for device in connected_devices() {
        let help = if device.user_label.is_empty() {
            device.product.clone()
        } else {
            format!("{} ({})", device.product, device.user_label)
        };
        for v in value(&device).into_iter().filter(|v| !v.is_empty()) {
            let helps = values.entry(v).or_default();
            if !helps.contains(&help) {
                helps.push(help.clone());
            }
        }
    }
    values
        .into_iter()
        .map(|(v, helps)| CompletionCandidate::new(v).help(Some(helps.join(", ").into())))
        .collect()
}

pub(crate) fn serials() -> Vec<CompletionCandidate> {
    candidates(|d| d.serials.clone())
}

pub(crate) fn labels() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.user_label.clone()])
}

pub(crate) fn products() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.product.clone()])
}

pub(crate) fn manufacturers() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.manufacturer.clone()])
}

pub(crate) fn apis() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.api
            .iter()
            .flat_map(|api| [api.gid.clone(), format!("{}@^{}", api.gid, api.version)])
            .collect()
    })
}

pub(crate) fn vid_pids() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.usb
            .iter()
            .map(|u| format!("{:04x}:{:04x}", u.vid, u.pid))
            .collect()
    })
}

pub(crate) fn usb_paths() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.usb
            .iter()
            .map(|u| {
                let ports: Vec<_> = u.port_chain.iter().map(|p| p.to_string()).collect();
                format!("{}-{}", u.bus_id, ports.join("."))
            })
            .collect()
    })
}
