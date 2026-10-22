//! A connection's own steering (CLT-46): every direct connection to a device serving `ww_timebase::Clock` with the
//! `host_sync` group keeps that clock on the PC's UTC, as the ww hub does for the devices it holds (SRV-31).
//!
//! On by default ([ClientConfig::clock_sync](crate::ClientConfig::clock_sync), [ENV]); off while the connection goes
//! through the hub, which steers. The task holds the connection weakly: it never keeps it open, and ends with the
//! program's last [Commander](crate::Commander) of it.

use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, info};
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ApiLevelLocationOwned, ApiLevelOwned};
use ww_timebase::SteererOwned;

use super::sync::{Conn, first_reading, run};
use super::{Options, RateCache, Status, steerer};
use crate::event_loop::commander::WeakCommander;

/// `off` (or `0`, `no`) turns a connection's own steering off where its [ClientConfig](crate::ClientConfig) doesn't
/// say; `on` or unset: on.
pub const ENV: &str = "WW_CLOCK_SYNC";

/// [ENV], `true` when not set; a value that doesn't parse is reported and taken as on.
pub(crate) fn enabled_from_env() -> bool {
    match std::env::var(ENV) {
        Ok(s) => match s.trim().to_ascii_lowercase().as_str() {
            "off" | "0" | "no" | "false" => false,
            "on" | "1" | "yes" | "true" | "" => true,
            other => {
                tracing::warn!("{ENV}={other}: expected on or off, steering stays on");
                true
            }
        },
        Err(_) => true,
    }
}

/// Where a connection's own steering is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Reading the device's clock the first time
    Starting,
    /// Steering: [ClockSteering::status] says how it goes
    Steering,
    /// Readings go on, no steer points ([ClockSteering::set_paused]): the device coasts in holdover
    Paused,
    /// The connection goes through the ww hub, which steers the device (SRV-31); steering starts again when a
    /// reconnect reaches the device directly
    ThroughHub,
    /// The device has no `ww_timebase::Clock` with `host_sync`
    Unsupported,
    /// The clock is there, but can't be steered (its time is not an integer, a call failed at the start)
    Failed(String),
}

/// A connection's own steering, from [Commander::clock_sync](crate::Commander::clock_sync): its phase and status, and
/// a pause. Clones share it; it stops with the last [Commander](crate::Commander) of the connection.
#[derive(Clone)]
pub struct ClockSteering {
    shared: Arc<Shared>,
}

pub(crate) struct Shared {
    phase: watch::Receiver<Phase>,
    status: watch::Receiver<Status>,
    paused: watch::Sender<bool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        if let Some(task) = self.task.lock().unwrap_or_else(|e| e.into_inner()).take() {
            task.abort();
        }
    }
}

impl ClockSteering {
    pub(crate) fn from_shared(shared: Arc<Shared>) -> Self {
        ClockSteering { shared }
    }

    /// Where it is now.
    pub fn phase(&self) -> Phase {
        let phase = self.shared.phase.borrow().clone();
        match phase {
            Phase::Steering if *self.shared.paused.borrow() => Phase::Paused,
            phase => phase,
        }
    }

    /// Phase changes as they come (without [Phase::Paused], which is [ClockSteering::set_paused]'s own).
    pub fn watch_phase(&self) -> watch::Receiver<Phase> {
        self.shared.phase.clone()
    }

    /// The latest status of the sync, [Status::default] before it steers.
    pub fn status(&self) -> Status {
        self.shared.status.borrow().clone()
    }

    /// Status updates as they come.
    pub fn watch(&self) -> watch::Receiver<Status> {
        self.shared.status.clone()
    }

    /// Pauses steering, or goes on: the clock is still read, but no steer point is sent, so the device coasts in
    /// holdover. For a program that has the device to itself for a while and wants it left alone (a firmware update,
    /// as the hub pauses under a device-wide claim, SRV-32).
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.send_replace(paused);
    }
}

/// What [start] needs to know of the connection.
pub(crate) struct Start {
    pub(crate) options: Options,
    /// The device's API, if known: steering starts only when it has the clock, without it `now()` is tried
    pub(crate) bundle: Option<Arc<ApiBundleOwned>>,
    /// The device's key (`usb:..`, `rtt:..`), for its [RateCache]
    pub(crate) key: Option<String>,
    /// The connection's client meta as sent, for the steerer's name and id
    pub(crate) meta: Option<Box<ww_link::ClientMetaOwned>>,
    /// Whether the connection goes through the hub, changing with reconnects
    pub(crate) via_hub: watch::Receiver<bool>,
}

/// The connection's steering, before its task runs (it needs a [WeakCommander] of the Commander this goes into).
pub(crate) fn prepare() -> (Arc<Shared>, Senders) {
    let (phase_tx, phase) = watch::channel(Phase::Starting);
    let (status_tx, status) = watch::channel(Status::default());
    let (paused, _) = watch::channel(false);
    let shared = Arc::new(Shared {
        phase,
        status,
        paused,
        task: Mutex::new(None),
    });
    (
        shared,
        Senders {
            phase: phase_tx,
            status: status_tx,
        },
    )
}

pub(crate) struct Senders {
    phase: watch::Sender<Phase>,
    status: watch::Sender<Status>,
}

/// Starts the steering task of `shared` on the connection of `weak`, on `rt`.
pub(crate) fn start(
    rt: &tokio::runtime::Handle,
    shared: &Arc<Shared>,
    senders: Senders,
    weak: WeakCommander,
    start: Start,
) {
    let paused = shared.paused.subscribe();
    let task = rt.spawn(steer(weak, start, senders, paused));
    *shared.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

async fn steer(weak: WeakCommander, start: Start, senders: Senders, paused: watch::Receiver<bool>) {
    let Start {
        mut options,
        bundle,
        key,
        meta,
        mut via_hub,
    } = start;
    if let Some(bundle) = &bundle
        && !serves_host_sync(bundle)
    {
        senders.phase.send_replace(Phase::Unsupported);
        return;
    }
    // the device sees the same program, pid and id as the connection's
    let steerer = match (options.steerer.take(), meta) {
        (Some(s), _) => s,
        (None, Some(m)) if m.instance_id != 0 => SteererOwned {
            id: m.instance_id,
            host: m.host,
            program: m.program,
            pid: m.pid,
        },
        (None, _) => steerer(None),
    };
    if options.rate_cache.is_none() {
        options.rate_cache = key
            .as_deref()
            .and_then(rate_key)
            .and_then(|serial| RateCache::for_device(&serial).ok());
    }
    loop {
        if *via_hub.borrow_and_update() {
            senders.phase.send_replace(Phase::ThroughHub);
            if via_hub.wait_for(|v| !*v).await.is_err() {
                return;
            }
            continue;
        }
        senders.phase.send_replace(Phase::Starting);
        let device_time = {
            let Some(cmd) = weak.upgrade() else {
                return;
            };
            first_reading(&cmd).await
        };
        let device_time = match device_time {
            Ok(t) => t,
            Err(e) => {
                let unsupported = bundle.is_none()
                    && (e.is_unimplemented() || matches!(e, crate::Error::RemoteError(_)));
                let phase = match unsupported {
                    true => Phase::Unsupported,
                    false => Phase::Failed(format!("{e:#}")),
                };
                debug!("not steering the device's clock: {phase:?}");
                senders.phase.send_replace(phase);
                return;
            }
        };
        info!("steering the device's clock onto UTC");
        senders.phase.send_replace(Phase::Steering);
        let conn = Conn::Weak(weak.clone());
        tokio::select! {
            _ = run(conn, device_time, options.clone(), steerer.clone(), paused.clone(), &senders.status) => return,
            r = via_hub.wait_for(|v| *v) => if r.is_err() { return },
        }
        info!("the connection goes through the ww hub now, which steers the device's clock");
    }
}

/// What the rate cache knows the device behind `key` (`usb:<vid>:<pid>:<serial>[:<iface>]`, `rtt:<probe>:<chip>`)
/// by: the same for a direct connection, the hub and `ww sync`; `None` for a device without a serial.
pub fn rate_key(key: &str) -> Option<String> {
    let (scheme, rest) = key.split_once(':')?;
    match scheme {
        "usb" => rest
            .split(':')
            .nth(2)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        "rtt" => Some(rest.replace(':', "-")),
        _ => None,
    }
}

/// The device's API has `ww_timebase::Clock` somewhere with its `host_sync` group (the trait's only optional items,
/// so a `Clock` that leaves out none has it).
pub fn serves_host_sync(bundle: &ApiBundleOwned) -> bool {
    fn search(bundle: &ApiBundleOwned, level: &ApiLevelOwned, depth: usize) -> bool {
        if depth > 16 {
            return false;
        }
        level.items.iter().any(|item| {
            let ApiItemKindOwned::Trait { trait_idx, omitted } = &item.kind else {
                return false;
            };
            let none_omitted = omitted.as_ref().is_none_or(|o| o.is_empty());
            match bundle.traits.get(trait_idx.0 as usize) {
                Some(ApiLevelLocationOwned::InLine { level, crate_idx }) => {
                    match is_clock(bundle, crate_idx.0, &level.trait_name) {
                        true => none_omitted,
                        false => search(bundle, level, depth + 1),
                    }
                }
                Some(ApiLevelLocationOwned::SkippedFullVersion {
                    crate_idx,
                    trait_name,
                    ..
                }) => none_omitted && is_clock(bundle, crate_idx.0, trait_name),
                _ => false,
            }
        })
    }
    fn is_clock(bundle: &ApiBundleOwned, crate_idx: u32, name: &str) -> bool {
        name == "Clock"
            && bundle
                .ext_crates
                .get(crate_idx as usize)
                .is_some_and(|c| c.crate_id == "ww_timebase")
    }
    search(bundle, &bundle.root, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_keys() {
        assert_eq!(
            rate_key("usb:c0de:cafe:B135-0042").as_deref(),
            Some("B135-0042")
        );
        assert_eq!(
            rate_key("usb:c0de:cafe:B135-0042:2").as_deref(),
            Some("B135-0042")
        );
        assert_eq!(rate_key("usb:c0de:cafe:"), None);
        assert_eq!(
            rate_key("rtt:0670FF:STM32H725").as_deref(),
            Some("0670FF-STM32H725")
        );
        assert_eq!(rate_key("inproc:hub/x"), None);
    }
}
