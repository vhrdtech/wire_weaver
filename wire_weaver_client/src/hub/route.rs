//! Which way `connect()` reaches a device it selected: directly or through the hub (CLT-39).

use tracing::{debug, info};
use ww_hub_api::DeviceStateOwned;
use ww_local_socket::Target;

use super::{HubClient, HubMode, MODE_ENV, ensure_running, hub_running, record_open};
use crate::ClientKind;
use crate::config::ValidatedConfig;
use crate::holders::{self, Mode};
use crate::local_socket::{Tunnel, open};

/// See [route].
pub(crate) enum Route {
    /// Open the device directly, as without a hub; `note` says why when a hub was in play (also logged).
    Direct { note: Option<String> },
    /// Through the hub: a tunnel to the device, its preamble answered
    Hub(Box<Tunnel>),
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Route::Direct { note } => write!(f, "Direct {{ note: {note:?} }}"),
            Route::Hub(t) => write!(f, "Hub({})", t.shown),
        }
    }
}

/// Picks the way to the device `key` (`usb:<vid>:<pid>:<serial>`, as the hub's registry names it) by
/// [ValidatedConfig::hub_mode]:
/// - `Direct`: directly, with a note when the hub holds it (the open then fails as busy, naming the hub).
/// - `Auto`: through the hub when its registry says it holds the device, or when this open trips the open counter
///   (then a hub is started if none runs); directly otherwise, and when the hub can't be had.
/// - `Prefer`: through the hub, started if needed; directly when none can start or it can't serve the device.
/// - `Required`: through the hub, started if needed, or an error.
pub(crate) async fn route(config: &ValidatedConfig, key: &str) -> anyhow::Result<Route> {
    let route = match config.hub_mode {
        HubMode::Direct => Route::Direct {
            note: holders::list()
                .into_iter()
                .any(|(k, h)| same_device(&k, key) && h.mode == Mode::Hub)
                .then(|| {
                    format!(
                        "the ww hub holds {key}, but this program connects directly ({MODE_ENV}=off or \
                         HubMode::Direct)"
                    )
                }),
        },
        HubMode::Auto => {
            if hub_running() && registry_holds(key).await {
                via_hub(key, HubMode::Auto, "the ww hub holds it").await?
            } else {
                let kind = match &config.client_meta {
                    Some(meta) => meta.kind,
                    None => crate::ClientMeta::new().resolve().kind,
                };
                let weight = if kind == ClientKind::Cli { 1.0 } else { 0.25 };
                match record_open(key, weight, config.hub_opens) {
                    Ok(opens) if opens.tripped => {
                        let why = format!(
                            "opened {} times within {:?}",
                            opens.weight, config.hub_opens.window
                        );
                        via_hub(key, HubMode::Auto, &why).await?
                    }
                    Ok(_) => Route::Direct { note: None },
                    Err(e) => {
                        debug!("ww hub: can't count opens of {key}: {e}");
                        Route::Direct { note: None }
                    }
                }
            }
        }
        mode @ (HubMode::Prefer | HubMode::Required) => {
            via_hub(key, mode, &format!("{MODE_ENV}={mode}")).await?
        }
    };
    if let Route::Direct { note: Some(note) } = &route {
        info!("{note}");
    }
    Ok(route)
}

async fn via_hub(key: &str, mode: HubMode, why: &str) -> anyhow::Result<Route> {
    let socket = match ensure_running(true).await {
        Ok(socket) => socket,
        Err(e) if mode == HubMode::Required => {
            anyhow::bail!("{key} through the ww hub ({why}): {e}")
        }
        Err(e) => {
            return Ok(Route::Direct {
                note: Some(format!("{e}; opening {key} directly")),
            });
        }
    };
    match open(&socket, Target::Device(key.to_string())).await {
        Ok(tunnel) => {
            debug!("{key} through the ww hub: {why}");
            Ok(Route::Hub(Box::new(tunnel)))
        }
        Err(e) if mode == HubMode::Required => {
            anyhow::bail!("{key} through the ww hub ({why}): {e}")
        }
        Err(e) => Ok(Route::Direct {
            note: Some(format!(
                "the ww hub can't serve {key} ({e}); opening it directly"
            )),
        }),
    }
}

/// Whether the running hub holds `key`: open by it, or held in its name (`mode = hub` in the lock file).
async fn registry_holds(key: &str) -> bool {
    let devices = async {
        let socket = ensure_running(false).await?;
        // boxed: connecting is what called us, a future can't contain itself
        let hub =
            Box::pin(HubClient::config(|c| c.hub_api_at(socket.display().to_string())).connect())
                .await?;
        anyhow::Ok(hub.registry().devices().call().await?)
    };
    match devices.await {
        Ok(devices) => devices.iter().any(|d| {
            same_device(&d.key, key)
                && match &d.state {
                    DeviceStateOwned::Open => true,
                    DeviceStateOwned::HeldElsewhere { holder } => holder.mode == "hub",
                    _ => false,
                }
        }),
        Err(e) => {
            debug!("ww hub: can't read its registry: {e:#}");
            false
        }
    }
}

/// A holder's key carries the interface number (`usb:..:<serial>:<iface>`), the registry's and ours don't.
fn same_device(holder_key: &str, key: &str) -> bool {
    holder_key == key
        || holder_key
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_numbers_are_the_same_device() {
        assert!(same_device("usb:c0de:cafe:1:2", "usb:c0de:cafe:1"));
        assert!(same_device("usb:c0de:cafe:1", "usb:c0de:cafe:1"));
        assert!(!same_device("usb:c0de:cafe:12", "usb:c0de:cafe:1"));
    }
}
