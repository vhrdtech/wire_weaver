//! Live link handover (CLT-44): a program holding a device directly hands the hub its live link, in two steps on its
//! hub API connection. `prepare` checks the snapshot and keeps the device's slot, so nobody opens the device meanwhile;
//! `commit` (the program let go of the device) opens it going on with the link, with the program's tunnel joined, and
//! waits [ATTACH_TIMEOUT] for that tunnel to connect.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, info};
use wire_weaver_client::adopt::{LinkSnapshotOwned, MediumOwned};
use ww_hub_api::AdoptErrorOwned;

use crate::Hub;
use crate::device::{self, DeviceHandle, Slot, ToDevice};
use crate::tunnel::TunnelQueue;

/// The program's tunnel must connect this soon after the commit, or the hub takes the program as gone: its requests
/// in flight are dropped, its streams closed on the device unless another client has them.
pub(crate) const ATTACH_TIMEOUT: Duration = Duration::from_secs(5);

use wire_weaver_client::adopt::CLIENT_VERSION;

/// An adoption prepared on a hub API connection, dropped with it.
pub(crate) struct Prepared {
    pub(crate) id: u64,
    snapshot: LinkSnapshotOwned,
    slot: Slot,
    /// The device's slot, held: a tunnel asking for the device meanwhile waits for the commit (or this to go)
    guard: tokio::sync::OwnedMutexGuard<Option<DeviceHandle>>,
}

/// A committed adoption whose tunnel hasn't connected yet.
pub(crate) struct Waiting {
    pub(crate) key: String,
    pub(crate) tunnel: u32,
    pub(crate) queue: Arc<TunnelQueue>,
    pub(crate) device: DeviceHandle,
}

pub(crate) async fn prepare(hub: &Arc<Hub>, bytes: &[u8]) -> Result<Prepared, AdoptErrorOwned> {
    let snapshot = LinkSnapshotOwned::decode(bytes).map_err(|why| {
        debug!("ww hub: adopt: bad snapshot: {why}");
        AdoptErrorOwned::Snapshot { why }
    })?;
    if !wire_weaver_client::adopt::same_minor(&snapshot.client_version, CLIENT_VERSION) {
        return Err(AdoptErrorOwned::Versions {
            hub: CLIENT_VERSION.into(),
        });
    }
    let key = snapshot.key.as_str();
    let scheme_fits = match (&snapshot.medium, key.split_once(':').map(|(s, _)| s)) {
        (MediumOwned::Usb { .. }, Some("usb")) | (MediumOwned::UsbSim { .. }, Some("usbsim")) => {
            true
        }
        _ => false,
    };
    if !scheme_fits || !device::has_transport(key) {
        return Err(AdoptErrorOwned::Snapshot {
            why: format!("the hub can't adopt {key} on this medium"),
        });
    }
    if hub.lent_to(key).is_some() {
        return Err(AdoptErrorOwned::Busy);
    }
    let slot = hub.device_slot(key);
    let guard = slot
        .clone()
        .try_lock_owned()
        .map_err(|_| AdoptErrorOwned::Busy)?;
    if guard.is_some() {
        // the hub holds it: whoever holds it directly can't (a stale snapshot)
        return Err(AdoptErrorOwned::Busy);
    }
    let id = loop {
        let id = wire_weaver_client::adopt::random_id();
        if id != 0 {
            break id;
        }
    };
    debug!("ww hub: adoption {id:#x} of {key} prepared");
    Ok(Prepared {
        id,
        snapshot,
        slot,
        guard,
    })
}

/// The program `client` (its hub API connection) let go of the device: opened with its link.
pub(crate) async fn commit(
    hub: &Arc<Hub>,
    client: u32,
    prepared: Prepared,
) -> Result<(), AdoptErrorOwned> {
    let Prepared {
        id,
        snapshot,
        slot,
        mut guard,
    } = prepared;
    let key = snapshot.key.clone();
    let tunnel = hub.client_like(client);
    let queue = TunnelQueue::new();
    let handle = match device::adopt(hub, &snapshot, slot, tunnel, queue.clone()).await {
        Ok(handle) => handle,
        Err(why) => {
            hub.client_left(tunnel);
            return Err(AdoptErrorOwned::Open { why });
        }
    };
    *guard = Some(handle.clone());
    drop(guard);
    info!(
        "ww hub: adopted the live link to {key} ({} requests in flight, {} streams)",
        snapshot.in_flight.len(),
        snapshot.streams.len()
    );
    hub.adoptions.lock().unwrap().insert(
        id,
        Waiting {
            key: key.clone(),
            tunnel,
            queue,
            device: handle,
        },
    );
    let hub = hub.clone();
    tokio::spawn(async move {
        tokio::time::sleep(ATTACH_TIMEOUT).await;
        let gone = hub.adoptions.lock().unwrap().remove(&id);
        if let Some(w) = gone {
            info!(
                "ww hub: the program whose link to {} was adopted didn't come back within {ATTACH_TIMEOUT:?}",
                w.key
            );
            let _ = w.device.tx.send(ToDevice::Leave { id: w.tunnel });
            hub.client_left(w.tunnel);
        }
    });
    Ok(())
}

/// The tunnel of adoption `id` connected for `key`: what it goes on with.
pub(crate) fn attach(hub: &Hub, id: u64, key: &str) -> Option<Waiting> {
    let mut adoptions = hub.adoptions.lock().unwrap();
    match adoptions.get(&id) {
        Some(w) if w.key == key => adoptions.remove(&id),
        _ => None,
    }
}
