//! A whole device to one program while the ww hub serves it to several (SRV-32): [Exclusive] for a firmware update or a
//! settings write, [Lend] for a tool that isn't a WireWeaver program (probe-rs, dfu-util). Both are claims on the
//! device's root (`ww_hub_api::Claims`), so they belong to the program's client instance and go with it when it exits
//! or crashes.

use std::path::Path;

use anyhow::{Context, bail};
use ww_hub_api::{ClaimEntryOwned, ClaimErrorOwned, DeviceStateOwned};

use super::{HubClient, ensure_running, hub_running};
use crate::{CallError, ClientMeta};

/// The whole device to this program: the hub answers every other program's calls and property writes with
/// `Common(InUse)` naming it, and doesn't steer the device's clock meanwhile. Their reads, streams and observing go on.
pub struct Exclusive {
    hub: HubClient,
    id: u32,
}

impl Exclusive {
    /// Takes the device `key` (`usb:<vid>:<pid>:<serial>`, as the hub's registry names it) when a hub of this user
    /// holds it, for `purpose` (shown to others). `None` when no hub runs or it doesn't hold the device: a program
    /// that reaches it then holds it directly, which is exclusive by itself.
    ///
    /// `meta` is the [ClientMeta] the program's device connection was made with
    /// ([ClientConfig::client_meta](crate::ClientConfig::client_meta)): a claim belongs to its instance id, so that
    /// connection's calls go through and everyone else's are refused.
    pub async fn take(
        key: &str,
        meta: &ClientMeta,
        purpose: &str,
    ) -> anyhow::Result<Option<Exclusive>> {
        if !hub_running() {
            return Ok(None);
        }
        let socket = ensure_running(false).await?;
        Self::take_at(&socket, key, meta, purpose).await
    }

    /// [Self::take] from the hub at `socket` (a hub run in another runtime dir, tests).
    pub async fn take_at(
        socket: &Path,
        key: &str,
        meta: &ClientMeta,
        purpose: &str,
    ) -> anyhow::Result<Option<Exclusive>> {
        let hub = connect(socket, Some(meta)).await?;
        let devices = hub
            .registry()
            .devices()
            .call()
            .await
            .context("reading the ww hub's devices")?;
        let held = devices
            .iter()
            .any(|d| same_device(&d.key, key) && d.state == DeviceStateOwned::Open);
        if !held {
            return Ok(None);
        }
        let id = hub
            .claims()
            .claim(key.to_string(), vec![], purpose.to_string(), None)
            .call()
            .await
            .map_err(|e| claim_error(key, e))?;
        Ok(Some(Exclusive { hub, id }))
    }

    /// The claim's id in the hub.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Gives the device back to everyone. Dropping it without does too, once the program's last connection to the
    /// hub closes.
    pub async fn release(self) -> anyhow::Result<()> {
        release(&self.hub, self.id).await
    }
}

/// A device lent to a program outside the hub: the hub closed it (its clients see their connection end, and new ones
/// are refused as lent) and opens it again when this is released, dropped, or the program exits.
pub struct Lend {
    hub: HubClient,
    id: u32,
}

impl Lend {
    /// Has the user's hub lend the device `key` for `purpose` (the command it is lent to, shown to others), and waits
    /// until it closed the device. `None` when no hub runs: nothing of the hub holds the device then.
    pub async fn take(key: &str, purpose: &str) -> anyhow::Result<Option<Lend>> {
        if !hub_running() {
            return Ok(None);
        }
        let socket = ensure_running(false).await?;
        Self::take_at(&socket, key, purpose).await.map(Some)
    }

    /// [Self::take] from the hub at `socket`.
    pub async fn take_at(socket: &Path, key: &str, purpose: &str) -> anyhow::Result<Lend> {
        let hub = connect(socket, None).await?;
        let id = hub
            .claims()
            .lend(key.to_string(), purpose.to_string(), None)
            .call()
            .await
            .map_err(|e| claim_error(key, e))?;
        Ok(Lend { hub, id })
    }

    /// Returns the device: the hub opens it again if it held it before.
    pub async fn release(self) -> anyhow::Result<()> {
        release(&self.hub, self.id).await
    }
}

async fn connect(socket: &Path, meta: Option<&ClientMeta>) -> anyhow::Result<HubClient> {
    let socket = socket.display().to_string();
    // boxed: a connect may be what called us, a future can't contain itself
    Box::pin(
        HubClient::config(|c| {
            let c = c.hub_api_at(socket.clone());
            match meta {
                Some(meta) => c.client_meta(meta.clone()),
                None => c,
            }
        })
        .connect(),
    )
    .await
    .context("connecting to the ww hub")
}

async fn release(hub: &HubClient, id: u32) -> anyhow::Result<()> {
    match hub.claims().release(id).call().await {
        Ok(()) => Ok(()),
        Err(CallError::Device(e)) => bail!("the ww hub: releasing claim {id}: {e:?}"),
        Err(e) => bail!("the ww hub: releasing claim {id}: {e:?}"),
    }
}

fn claim_error(key: &str, e: CallError<ClaimErrorOwned>) -> anyhow::Error {
    match e {
        CallError::Device(
            ClaimErrorOwned::InUse { claim } | ClaimErrorOwned::Overlap { claim },
        ) => {
            anyhow::anyhow!("{key} is in use: {}", describe_claim(&claim))
        }
        CallError::Device(e) => anyhow::anyhow!("the ww hub refused {key}: {e:?}"),
        e if e.is_unimplemented() => anyhow::anyhow!(
            "the running ww hub can't hand out whole devices yet (SRV-32): `ww hub stop` it, or let it go idle"
        ),
        e => anyhow::anyhow!("the ww hub: {key}: {e:?}"),
    }
}

/// `dg pid 4711 (flashing B135 #4), lent` as the claim says.
pub fn describe_claim(claim: &ClaimEntryOwned) -> String {
    let h = &claim.holder;
    let mut s = match (h.program.is_empty(), h.pid) {
        (true, 0) => format!("client {}", h.id),
        (true, pid) => format!("pid {pid}"),
        (false, 0) => h.program.clone(),
        (false, pid) => format!("{} pid {pid}", h.program),
    };
    if !claim.purpose.is_empty() {
        s += &format!(" ({})", claim.purpose);
    }
    if claim.lent {
        s += ", lent";
    }
    s
}

/// A registry key and ours name the same device (the registry's may carry the interface number).
fn same_device(registry_key: &str, key: &str) -> bool {
    registry_key == key
        || registry_key
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'))
}
