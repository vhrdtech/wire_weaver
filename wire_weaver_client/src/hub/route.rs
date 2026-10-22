//! Which way `connect()` reaches a device it selected: directly or through the hub (CLT-39).

use tracing::{debug, info};
use ww_hub_api::DeviceStateOwned;
use ww_local_socket::Target;

use super::{HubClient, HubMode, MODE_ENV, ensure_running, handover, hub_running, record_open};
use crate::ClientKind;
use crate::config::ValidatedConfig;
use crate::connection_event::STATUS_LENT;
use crate::holders::{self, Mode};
use crate::local_socket::{OpenError, Tunnel, open, refusal};

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
///
/// In every mode but `Direct`, a device another program holds directly is asked for first (CLT-43): a holder that
/// hands it to the hub is reached through the hub, one that refuses fails the connection naming it.
///
/// `count_opens`: [HubMode::Auto] counts opens of this device (USB); a device on a local socket isn't counted.
pub(crate) async fn route(
    config: &ValidatedConfig,
    key: &str,
    count_opens: bool,
) -> anyhow::Result<Route> {
    if config.hub_mode != HubMode::Direct
        && let Some(route) = ask_holder(config, key).await?
    {
        return Ok(route);
    }
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
            } else if !count_opens {
                Route::Direct { note: None }
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

/// Another program holds `key` directly and listens for a polite handover (CLT-43): with a hub to hand it to, asks
/// it. `None`: nobody to ask (or no hub), go on as without a holder (the open fails as busy, naming it).
async fn ask_holder(config: &ValidatedConfig, key: &str) -> anyhow::Result<Option<Route>> {
    let Some(holder) = tokio::task::spawn_blocking({
        let key = key.to_string();
        move || handover::direct_holder(&key)
    })
    .await
    .ok()
    .flatten() else {
        return Ok(None);
    };
    // a holder is only asked when there is a hub to take the device: it would lose it for nothing otherwise
    if let Err(e) = ensure_running(true).await {
        if config.hub_mode == HubMode::Required {
            anyhow::bail!("{key} through the ww hub: {e}");
        }
        info!("{key} is {holder}; not asking it to hand the device over: {e}");
        return Ok(None);
    }
    let program = holder.program.clone();
    let started = std::time::Instant::now();
    match handover::ask(holder, config.client_meta.as_deref()).await {
        handover::Asked::Released(holder) => {
            info!(
                "{program} (pid {}) handed {key} to the ww hub in {:?}",
                holder.pid,
                started.elapsed()
            );
            // the device is free now: through the hub whatever the mode, Auto included
            let mode = match config.hub_mode {
                HubMode::Required => HubMode::Required,
                _ => HubMode::Prefer,
            };
            Ok(Some(
                via_hub(key, mode, &format!("{program} handed it over")).await?,
            ))
        }
        handover::Asked::Refused(holder, meta) => {
            anyhow::bail!(handover::refused(key, &holder, meta.as_deref()))
        }
        handover::Asked::Failed(holder, why) => {
            anyhow::bail!("{key} is {holder}; asked to hand the device to the ww hub, {why}")
        }
    }
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
        // the hub holds the device, opening it directly would only find it busy: lent (it comes back, the caller waits)
        // or versions it doesn't serve (the error says what to do)
        Err(OpenError::Refused { shown, answer })
            if matches!(answer.status.to_u8(), 4 | STATUS_LENT) =>
        {
            Err(refusal(shown, answer).into())
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
pub(crate) fn same_device(holder_key: &str, key: &str) -> bool {
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

#[cfg(all(test, unix))]
mod route_tests {
    use super::*;
    use crate::ClientConfig;
    use crate::hub::start::tests::with_runtime_dir;

    fn config(mode: HubMode) -> ValidatedConfig {
        ClientConfig::new()
            .usb()
            .hub(mode)
            .client_meta(crate::ClientMeta::new().kind(ClientKind::Cli))
            .validate()
            .unwrap()
    }

    /// Runs `f` with no hub executable to be found: none named, none on `PATH`, none next to the test binary.
    fn without_hub_exe<R>(f: impl FnOnce() -> R) -> R {
        with_runtime_dir(|dir| {
            let path = std::env::var_os("PATH");
            // SAFETY: with_runtime_dir holds the environment lock
            unsafe { std::env::set_var("PATH", dir) };
            let r = f();
            if let Some(p) = path {
                // SAFETY: as above
                unsafe { std::env::set_var("PATH", p) };
            }
            r
        })
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn no_hub_executable_prefer_goes_direct() {
        without_hub_exe(|| {
            let route = block_on(route(&config(HubMode::Prefer), "usb:c0de:cafe:1", true)).unwrap();
            match route {
                Route::Direct { note: Some(note) } => {
                    assert!(note.contains("no ww hub executable"), "{note}")
                }
                other => panic!("expected direct with a note, got {other:?}"),
            }
        });
    }

    #[test]
    fn no_hub_executable_required_fails_and_says_how_to_get_one() {
        without_hub_exe(|| {
            let e =
                block_on(route(&config(HubMode::Required), "usb:c0de:cafe:1", true)).unwrap_err();
            assert!(
                e.to_string().contains("cargo install wire_weaver_cli"),
                "{e}"
            );
        });
    }

    #[test]
    fn auto_goes_direct_until_the_counter_trips() {
        without_hub_exe(|| {
            let c = config(HubMode::Auto);
            for _ in 0..2 {
                let r = block_on(route(&c, "usb:c0de:cafe:2", true)).unwrap();
                assert!(matches!(r, Route::Direct { note: None }), "{r:?}");
            }
            // the third shell-launched open wants the hub, finds none to start and goes direct
            let r = block_on(route(&c, "usb:c0de:cafe:2", true)).unwrap();
            assert!(matches!(r, Route::Direct { note: Some(_) }), "{r:?}");
        });
    }

    #[test]
    fn off_goes_direct_with_a_note_when_the_hub_holds_the_device() {
        with_runtime_dir(|_| {
            // as the hub holds a device it opened: `mode = hub`, the key with the interface number
            let _held = holders::claim("usb:c0de:cafe:3:0", Mode::Hub).unwrap();
            let r = block_on(route(&config(HubMode::Direct), "usb:c0de:cafe:3", true)).unwrap();
            match r {
                Route::Direct { note: Some(note) } => assert!(note.contains("holds"), "{note}"),
                other => panic!("expected direct with a note, got {other:?}"),
            }
            let r = block_on(route(&config(HubMode::Direct), "usb:c0de:cafe:4", true)).unwrap();
            assert!(matches!(r, Route::Direct { note: None }), "{r:?}");
        });
    }
}
