//! The devices the hub knows: WireWeaver USB devices plugged in (enumerated, not opened) and who holds them (the
//! CLT-38 lock files), with `Plugged` / `Unplugged` / `HolderChanged` events.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use futures_lite::StreamExt;
use tracing::debug;
use wire_weaver_client::holders::{self, DeviceRef, Holder};
use ww_hub_api::{DeviceEntryOwned, DeviceStateOwned, EventOwned, HolderOwned};

use crate::Hub;

/// Re-enumerated this often even without a hotplug event (a missed one, a lock file taken or released).
const RESCAN: Duration = Duration::from_secs(2);

/// Every device the hub can tell about now.
pub(crate) async fn entries(hub: &Hub) -> Vec<DeviceEntryOwned> {
    let usb = wire_weaver_client::list_usb_devices()
        .await
        .unwrap_or_default();
    // files and OS locks: off the hub's thread, its tunnels go on meanwhile
    let held = tokio::task::spawn_blocking(holders::list)
        .await
        .unwrap_or_default();
    let mut entries = BTreeMap::new();
    for d in &usb {
        let Some(loc) = &d.usb else { continue };
        let serial = d.serials.first().map(String::as_str).unwrap_or("");
        let key = holders::key_for(DeviceRef::Usb {
            vid: loc.vid,
            pid: loc.pid,
            serial,
            iface: None,
        });
        entries.insert(
            key.clone(),
            DeviceEntryOwned {
                key,
                product: d.product.clone(),
                serial: serial.to_string(),
                api: d.api.as_ref().map(|a| format!("{}@{}", a.gid, a.version)),
                state: DeviceStateOwned::Present,
                tunnels: 0,
                location: Some(d.location.clone()),
                label: Some(d.user_label.clone()).filter(|l| !l.is_empty()),
                api_hash: d
                    .api
                    .as_ref()
                    .filter(|a| !a.signature.hash.is_empty())
                    .map(|a| a.signature.to_string()),
            },
        );
    }
    for (key, holder) in held {
        // a holder's key carries the interface number (`usb:..:<serial>:<iface>`), enumeration doesn't
        let entry = entries
            .iter_mut()
            .find(|(k, _)| key == **k || key.starts_with(&format!("{k}:")))
            .map(|(_, e)| e);
        let state = DeviceStateOwned::HeldElsewhere {
            holder: holder_owned(&holder),
        };
        match entry {
            Some(e) => e.state = state,
            // e.g. an RTT probe: known from its lock file only
            None => {
                entries.insert(
                    key.clone(),
                    DeviceEntryOwned {
                        key,
                        product: String::new(),
                        serial: String::new(),
                        api: None,
                        state,
                        tunnels: 0,
                        location: None,
                        label: None,
                        api_hash: None,
                    },
                );
            }
        }
    }
    // what the hub holds itself: open, with its tunnels, whatever its own lock file says
    let open = hub.state.lock().unwrap().open.clone();
    for (key, tunnels) in open {
        let entry = entries
            .iter_mut()
            .find(|(k, _)| key == **k || key.starts_with(&format!("{k}:")))
            .map(|(_, e)| e);
        match entry {
            Some(e) => {
                e.state = DeviceStateOwned::Open;
                e.tunnels = tunnels;
            }
            None => {
                entries.insert(
                    key.clone(),
                    DeviceEntryOwned {
                        key,
                        product: String::new(),
                        serial: String::new(),
                        api: None,
                        state: DeviceStateOwned::Open,
                        tunnels,
                        location: None,
                        label: None,
                        api_hash: None,
                    },
                );
            }
        }
    }
    // lent out (SRV-32): closed by the hub, maybe held by a tool that writes no lock file
    let lent: Vec<_> = {
        let state = hub.state.lock().unwrap();
        state
            .claims
            .lent_out()
            .map(|c| (c.key.clone(), c.entry()))
            .collect()
    };
    for (key, claim) in lent {
        let state = DeviceStateOwned::Lent { claim };
        match entries
            .iter_mut()
            .find(|(k, _)| key == **k || key.starts_with(&format!("{k}:")))
        {
            Some((_, e)) => e.state = state,
            None => {
                entries.insert(
                    key.clone(),
                    DeviceEntryOwned {
                        key,
                        product: String::new(),
                        serial: String::new(),
                        api: None,
                        state,
                        tunnels: 0,
                        location: None,
                        label: None,
                        api_hash: None,
                    },
                );
            }
        }
    }
    entries.into_values().collect()
}

pub(crate) fn holder_owned(h: &Holder) -> HolderOwned {
    HolderOwned {
        pid: h.pid,
        program: h.program.clone(),
        exe: h.exe.clone(),
        mode: h.mode.to_string(),
        since_ms: h
            .since
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    }
}

/// Watches for plugs, unplugs and holder changes for as long as the hub runs, sending them as events.
pub(crate) async fn watch(hub: Arc<Hub>) {
    let mut hotplug = match nusb::watch_devices() {
        Ok(w) => Some(w),
        Err(e) => {
            debug!("ww hub: no hotplug events ({e}), rescanning every {RESCAN:?}");
            None
        }
    };
    let mut known: BTreeMap<String, DeviceStateOwned> = entries(&hub)
        .await
        .into_iter()
        .map(|e| (e.key, e.state))
        .collect();
    loop {
        let wait = async {
            match hotplug.as_mut() {
                Some(h) => {
                    let _ = tokio::time::timeout(RESCAN, h.next()).await;
                    // a plug comes with several events, and the device needs a moment to enumerate
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                None => tokio::time::sleep(RESCAN).await,
            }
        };
        wait.await;
        let now: BTreeMap<String, DeviceStateOwned> = entries(&hub)
            .await
            .into_iter()
            .map(|e| (e.key, e.state))
            .collect();
        for (key, state) in &now {
            match known.get(key) {
                None => {
                    let _ = hub.events.send(EventOwned::Plugged { key: key.clone() });
                }
                Some(old) if old != state => {
                    let _ = hub
                        .events
                        .send(EventOwned::HolderChanged { key: key.clone() });
                }
                Some(_) => {}
            }
        }
        for key in known.keys() {
            if !now.contains_key(key) {
                let _ = hub.events.send(EventOwned::Unplugged { key: key.clone() });
            }
        }
        known = now;
    }
}
