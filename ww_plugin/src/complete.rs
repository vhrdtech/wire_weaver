//! Completion candidates for the device selection flags ([`crate::DeviceArgs`]), listed live from connected devices
//! without opening them. Completion must never fail loudly: any error gives no candidates.

use clap_complete::CompletionCandidate;
use std::collections::BTreeMap;
use wire_weaver_client::DeviceInfo;

/// Devices reporting a WireWeaver API id, empty on any error.
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
pub fn candidates(value: impl Fn(&DeviceInfo) -> Vec<String>) -> Vec<CompletionCandidate> {
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

/// Serial numbers of WireWeaver devices of RP2040 / RP2350 boot ROMs in BOOTSEL mode (`ww dfu rp`) and of STM32 bootloaders in DFU mode (`ww dfu stm32`).
pub fn serials() -> Vec<CompletionCandidate> {
    let mut all = candidates(|d| d.serials.clone());
    if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        let bootsel = rt
            .block_on(wire_weaver_client::ww_picoboot::usb::list())
            .unwrap_or_default();
        let stm32 = rt
            .block_on(wire_weaver_client::ww_stm32_dfu::usb::list())
            .unwrap_or_default();
        for d in stm32 {
            if let Some(serial) = d.serial_number() {
                let help = format!("{} (STM32 DFU)", d.product_string().unwrap_or("bootloader"));
                all.push(CompletionCandidate::new(serial).help(Some(help.into())));
            }
        }
        for d in bootsel {
            if let Some(serial) = d.serial_number() {
                let help = format!("{} (BOOTSEL)", d.product_string().unwrap_or("boot ROM"));
                all.push(CompletionCandidate::new(serial).help(Some(help.into())));
            }
        }
    }
    all
}

/// Serial numbers and user labels of WireWeaver devices (`ww sync DEVICE...`).
pub fn serials_and_labels() -> Vec<CompletionCandidate> {
    candidates(|d| {
        let mut v = d.serials.clone();
        v.push(d.user_label.clone());
        v
    })
}

/// Device keys of `ww hub who`, as the hub names them (`usb:<vid>:<pid>:<serial>`, `rtt:..` from lock files), with
/// product and label as help.
pub fn device_keys() -> Vec<CompletionCandidate> {
    use wire_weaver_client::holders::{self, DeviceRef};
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for d in connected_devices() {
        let Some(u) = &d.usb else { continue };
        let serial = d.serials.first().map(String::as_str).unwrap_or("");
        let key = holders::key_for(DeviceRef::Usb {
            vid: u.vid,
            pid: u.pid,
            serial,
            iface: None,
        });
        let help = if d.user_label.is_empty() {
            d.product.clone()
        } else {
            format!("{} ({})", d.product, d.user_label)
        };
        out.insert(key, help);
    }
    for (key, holder) in holders::list() {
        let base = key
            .rsplit_once(':')
            .filter(|(_, i)| i.parse::<u8>().is_ok());
        let known = out.contains_key(&key) || base.is_some_and(|(b, _)| out.contains_key(b));
        if !known {
            out.insert(key, format!("held by {}", holder.program));
        }
    }
    out.into_iter()
        .map(|(k, h)| CompletionCandidate::new(k).help(Some(h.into())))
        .collect()
}

/// Debug probes for `--probe`: serial numbers, and VID:PID for a probe without one, with the probe's name as help.
pub fn probes() -> Vec<CompletionCandidate> {
    wire_weaver_client::list_probes()
        .into_iter()
        .map(|p| {
            let vid_pid = format!("{:04x}:{:04x}", p.vid, p.pid);
            match p.serial {
                Some(serial) => CompletionCandidate::new(serial)
                    .help(Some(format!("{} ({vid_pid})", p.name).into())),
                None => CompletionCandidate::new(vid_pid).help(Some(p.name.into())),
            }
        })
        .collect()
}

/// probe-rs chip names for `--rtt` starting with what is typed so far (a whole list is several thousand names).
pub fn rtt_chips(current: &std::ffi::OsStr) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return vec![];
    };
    wire_weaver_client::rtt_chips(current)
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

pub fn labels() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.user_label.clone()])
}

pub fn products() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.product.clone()])
}

pub fn manufacturers() -> Vec<CompletionCandidate> {
    candidates(|d| vec![d.manufacturer.clone()])
}

pub fn apis() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.api
            .iter()
            .flat_map(|api| [api.gid.clone(), format!("{}@^{}", api.gid, api.version)])
            .collect()
    })
}

pub fn vid_pids() -> Vec<CompletionCandidate> {
    candidates(|d| {
        d.usb
            .iter()
            .map(|u| format!("{:04x}:{:04x}", u.vid, u.pid))
            .collect()
    })
}

pub fn usb_paths() -> Vec<CompletionCandidate> {
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
