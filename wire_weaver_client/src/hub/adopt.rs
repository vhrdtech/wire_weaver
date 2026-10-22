//! The holder's side of a live link handover (CLT-44): a program holding a device directly hands its live link to the
//! user's hub (`ww_hub_api::Adopt`) and goes on through a tunnel, the device never seeing a reconnect. The event loop
//! drives it (`event_loop::live`); this is the talk with the hub.

use std::path::PathBuf;

use ww_hub_api::AdoptErrorOwned;
use ww_local_socket::{Preamble, Target};

use super::{HubClient, ensure_running};
use crate::CallError;
use crate::adopt::LinkSnapshotOwned;
use crate::local_socket::{OpenError, Tunnel};

/// Why a step with the hub didn't go through.
#[derive(Debug)]
pub(crate) enum Refused {
    /// The hub answered no (another version, the device busy, a hub before CLT-44): the program hands the device over
    /// by releasing it instead (CLT-43)
    ByHub(String),
    /// The hub couldn't be asked or went away: the program goes on holding the device
    Gone(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::ByHub(why) => write!(f, "the ww hub refused: {why}"),
            Refused::Gone(why) => write!(f, "the ww hub is gone: {why}"),
        }
    }
}

/// A connection to the user's hub API for one adoption: what it prepared lives as long as this does.
pub(crate) struct HubSide {
    hub: HubClient,
    socket: PathBuf,
}

/// Connects to the user's running hub (the program asking for the device started it).
pub(crate) async fn connect() -> Result<HubSide, Refused> {
    let socket = ensure_running(false)
        .await
        .map_err(|e| Refused::ByHub(e.to_string()))?;
    let at = socket.display().to_string();
    // the first connect of a process parses the embedded API snapshots (~90 ms in a debug build): not on the
    // runtime's workers, where it would hold up the link that is still running
    let _ = tokio::task::spawn_blocking(|| crate::snapshots::all().len()).await;
    let hub = connect_hub(at)
        .await
        .map_err(|e| Refused::ByHub(format!("{e:#}")))?;
    Ok(HubSide { hub, socket })
}

/// A trait object with its type in the signature: this runs inside an event loop, and the hub client's connect starts
/// one, so the future's type can't contain itself.
fn connect_hub(
    at: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<HubClient>> + Send>> {
    Box::pin(HubClient::config(move |c| c.hub_api_at(at.clone())).connect())
}

fn refused(e: CallError<AdoptErrorOwned>) -> Refused {
    match e {
        CallError::Device(e) => Refused::ByHub(format!("{e:?}")),
        CallError::Unimplemented { .. } => {
            Refused::ByHub("the hub is older than live link handover".into())
        }
        CallError::Client(e) => Refused::Gone(e.to_string()),
    }
}

impl HubSide {
    /// `Adopt::prepare`: the id for [Self::commit] and [Self::tunnel].
    pub(crate) async fn prepare(&self, snapshot: &LinkSnapshotOwned) -> Result<u64, Refused> {
        let bytes = snapshot
            .encode()
            .map_err(|e| Refused::ByHub(format!("snapshot: {e}")))?;
        self.hub
            .adopt()
            .prepare(bytes)
            .call()
            .await
            .map_err(refused)
    }

    /// `Adopt::commit`, once this program let go of the device.
    pub(crate) async fn commit(&self, id: u64) -> Result<(), Refused> {
        self.hub.adopt().commit(id).call().await.map_err(refused)
    }

    /// The tunnel this program goes on through, to the adopted link.
    pub(crate) async fn tunnel(&self, key: &str, id: u64) -> Result<Tunnel, OpenError> {
        let preamble = Preamble::new(Target::Device(key.to_string())).with_adopt(id);
        crate::local_socket::open_preamble(&self.socket, preamble).await
    }
}
