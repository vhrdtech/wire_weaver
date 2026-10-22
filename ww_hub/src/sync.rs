//! Clock sync in the hub (SRV-31): every device the hub holds open that serves `ww_timebase::Clock` with the
//! `host_sync` group is steered onto this PC's UTC by a `wire_weaver_client::clock_sync::ClockSync` on the hub's own connection to it,
//! and kept open for [Steering::linger] after its last client left, so short-lived commands don't leave it coasting.
//!
//! One task per open device ([start]): it looks the clock up in the device's introspection data, starts and stops
//! the sync as steering is switched on and off ([ww_hub_api::ClockSync::set_enabled]), and turns the sync's status
//! into the [SyncEntryOwned] that `ClockSync::devices` lists and `ClockSync::stats` streams.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{debug, info};
use wire_weaver_client::clock_sync::{
    ClockSync, Options, RateCache, Status, rate_key, serves_host_sync,
};
use ww_hub_api::{SyncEntryOwned, SyncPhaseOwned};

use crate::Hub;
use crate::device::{Device, ToDevice};

/// `ClockSync::stats` sends a device's entry at most this often (the sync's status changes with every reading).
const STATS_EVERY: Duration = Duration::from_millis(250);
/// How long the device's introspection data may take.
const BUNDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// The hub's steering settings and what each device's sync is doing.
pub(crate) struct Steering {
    /// Steering on for every device unless switched off for it (`WW_HUB_SYNC`, `set_enabled(None, ..)`)
    enabled: bool,
    /// Devices steering is switched off for while `enabled`
    off: BTreeSet<String>,
    /// Devices it is switched on for while not `enabled`
    on: BTreeSet<String>,
    /// How long a steered device stays open after its last client left; `None` forever
    pub(crate) linger: Option<Duration>,
    entries: BTreeMap<String, SyncEntryOwned>,
    tasks: BTreeMap<String, JoinHandle<()>>,
    /// Bumped on every change of the settings: the tasks look again
    changed: watch::Sender<u64>,
}

impl Steering {
    pub(crate) fn new(enabled: bool, linger: Option<Duration>) -> Self {
        Steering {
            enabled,
            off: BTreeSet::new(),
            on: BTreeSet::new(),
            linger,
            entries: BTreeMap::new(),
            tasks: BTreeMap::new(),
            changed: watch::channel(0).0,
        }
    }

    fn enabled_for(&self, key: &str) -> bool {
        match self.enabled {
            true => !self.off.contains(key),
            false => self.on.contains(key),
        }
    }

    /// `ClockSync::set_enabled`.
    pub(crate) fn set_enabled(&mut self, key: Option<&str>, on: bool) {
        match key {
            None => {
                self.enabled = on;
                self.off.clear();
                self.on.clear();
            }
            Some(key) => {
                let (add, remove) = match on {
                    true => (&mut self.on, &mut self.off),
                    false => (&mut self.off, &mut self.on),
                };
                remove.remove(key);
                if on != self.enabled {
                    add.insert(key.to_string());
                }
            }
        }
        self.changed.send_modify(|n| *n += 1);
    }

    /// Something the tasks look at changed elsewhere (a device-wide claim came or went, SRV-32): they look again.
    pub(crate) fn poke(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    /// The device `key` is being steered now: it stays open for [Steering::linger].
    pub(crate) fn steering(&self, key: &str) -> bool {
        self.entries
            .get(key)
            .is_some_and(|e| e.phase == SyncPhaseOwned::Steering)
    }

    pub(crate) fn entries(&self) -> Vec<SyncEntryOwned> {
        self.entries.values().cloned().collect()
    }
}

/// Starts the task that steers the device `key` the hub just opened; it ends with [stop].
pub(crate) fn start(
    hub: &Arc<Hub>,
    key: &str,
    device: Arc<Device>,
    tx: &mpsc::UnboundedSender<ToDevice>,
) {
    let task = tokio::spawn(run(hub.clone(), key.to_string(), device, tx.downgrade()));
    let mut steering = hub.steering.lock().unwrap();
    steering
        .entries
        .insert(key.to_string(), entry(key, SyncPhaseOwned::Starting));
    if let Some(old) = steering.tasks.insert(key.to_string(), task) {
        old.abort();
    }
}

/// The device `key` was closed: its sync stops (the device goes on in holdover), its entry goes.
pub(crate) fn stop(hub: &Hub, key: &str) {
    let mut steering = hub.steering.lock().unwrap();
    if let Some(task) = steering.tasks.remove(key) {
        task.abort();
    }
    steering.entries.remove(key);
}

async fn run(
    hub: Arc<Hub>,
    key: String,
    device: Arc<Device>,
    tx: mpsc::WeakUnboundedSender<ToDevice>,
) {
    let publish = |phase: SyncPhaseOwned, status: Option<&Status>| {
        let mut e = entry(&key, phase);
        if let Some(s) = status {
            fill(&mut e, s);
        }
        hub.steering
            .lock()
            .unwrap()
            .entries
            .insert(key.clone(), e.clone());
        let _ = hub.sync_stats.send(e);
    };
    // the device's linger depends on whether it is steered: its task looks again
    let recheck = || {
        if let Some(tx) = tx.upgrade() {
            let _ = tx.send(ToDevice::Recheck);
        }
    };
    let bundle = match tx.upgrade() {
        Some(tx) => {
            let (reply, bundle) = oneshot::channel();
            let _ = tx.send(ToDevice::Bundle { reply });
            drop(tx);
            tokio::time::timeout(BUNDLE_TIMEOUT, bundle)
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
        }
        None => return,
    };
    // without introspection data the clock is tried: `now()` fails on a device without one
    if let Some(bundle) = &bundle
        && !serves_host_sync(bundle)
    {
        debug!("ww hub: {key} has no ww_timebase::Clock with host_sync");
        publish(SyncPhaseOwned::Unsupported, None);
        return;
    }
    let mut changed = hub.steering.lock().unwrap().changed.subscribe();
    loop {
        changed.mark_unchanged();
        if !hub.steering.lock().unwrap().enabled_for(&key) {
            publish(SyncPhaseOwned::Off, None);
            recheck();
            if changed.changed().await.is_err() {
                return;
            }
            continue;
        }
        let options = Options {
            steerer: Some(hub.steerer.clone()),
            rate_cache: hub
                .state_dir
                .as_deref()
                .zip(rate_key(&key))
                .map(|(dir, serial)| RateCache::in_dir(dir, &serial)),
            ..hub.sync_options.clone()
        };
        let sync = match ClockSync::start(&device.cmd, options).await {
            Ok(sync) => sync,
            Err(e) => {
                let why = format!("{e:#}");
                debug!("ww hub: not steering {key}: {why}");
                // a device without the clock answers the call with an error of its own
                let phase = match bundle.is_none() && why.contains("Unimplemented") {
                    true => SyncPhaseOwned::Unsupported,
                    false => SyncPhaseOwned::Failed { why },
                };
                publish(phase, None);
                return;
            }
        };
        info!("ww hub: steering {key}");
        let mut status = sync.watch();
        publish(SyncPhaseOwned::Steering, Some(&status.borrow_and_update()));
        recheck();
        let mut last_sent = Instant::now();
        // a program with the whole device to itself (a root claim, a lend; SRV-32): readings go on, steer points
        // don't, the device coasts in holdover
        let mut paused = false;
        let pause = |paused: &mut bool, status: &Status| {
            let now = hub.steering_paused(&key);
            if now != *paused {
                *paused = now;
                sync.set_paused(now);
                info!(
                    "ww hub: steering {key} {}",
                    if now { "paused" } else { "resumed" }
                );
                let phase = match now {
                    true => SyncPhaseOwned::Paused,
                    false => SyncPhaseOwned::Steering,
                };
                publish(phase, Some(status));
            }
        };
        pause(&mut paused, &status.borrow());
        loop {
            tokio::select! {
                r = status.changed() => {
                    if r.is_err() {
                        return;
                    }
                    let phase = match paused {
                        true => SyncPhaseOwned::Paused,
                        false => SyncPhaseOwned::Steering,
                    };
                    let mut e = entry(&key, phase);
                    fill(&mut e, &status.borrow_and_update());
                    hub.steering.lock().unwrap().entries.insert(key.clone(), e.clone());
                    if last_sent.elapsed() >= STATS_EVERY {
                        last_sent = Instant::now();
                        let _ = hub.sync_stats.send(e);
                    }
                }
                r = changed.changed() => {
                    if r.is_err() {
                        return;
                    }
                    if !hub.steering.lock().unwrap().enabled_for(&key) {
                        info!("ww hub: steering {key} switched off");
                        break;
                    }
                    pause(&mut paused, &status.borrow());
                }
            }
        }
        // the device goes on in holdover
        drop(sync);
    }
}

fn entry(key: &str, phase: SyncPhaseOwned) -> SyncEntryOwned {
    SyncEntryOwned {
        key: key.to_string(),
        phase,
        state: None,
        source: None,
        steerer: None,
        noted: None,
        followed: None,
        rate_ppb: None,
        rate_from_cache: false,
        min_rtt_ns: None,
        residual_ns: None,
        check_rtt_ns: None,
        error_us: None,
        slew_us: None,
        samples: 0,
        steers: 0,
        restarts: 0,
        error: None,
    }
}

/// The sync's status into the entry.
fn fill(e: &mut SyncEntryOwned, s: &Status) {
    let device = s.device.as_ref();
    e.state = device.map(|d| d.state);
    e.source = s.source;
    e.steerer = device.and_then(|d| d.steerer.clone());
    e.noted = device.and_then(|d| d.noted.clone());
    e.followed = s.followed;
    e.rate_ppb = s.fit.map(|f| f.rate_ppb());
    e.rate_from_cache = s.fit.is_some_and(|f| f.prior);
    e.min_rtt_ns = s.fit.map(|f| f.min_rtt);
    e.residual_ns = s.residual;
    e.check_rtt_ns = s.check_rtt;
    e.error_us = device.and_then(|d| d.error).map(|err| {
        err.to_f64_in(ww_si::Prefix::Micro)
            .clamp(0.0, u32::MAX as f64) as u32
    });
    e.slew_us = device.and_then(|d| d.slew_us);
    e.samples = s.samples as u32;
    e.steers = s.steers;
    e.restarts = s.restarts;
    e.error = s.error.clone();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_per_device_and_globally() {
        let mut s = Steering::new(true, None);
        assert!(s.enabled_for("a"));
        s.set_enabled(Some("a"), false);
        assert!(!s.enabled_for("a") && s.enabled_for("b"));
        s.set_enabled(None, false);
        assert!(!s.enabled_for("a") && !s.enabled_for("b"));
        s.set_enabled(Some("b"), true);
        assert!(!s.enabled_for("a") && s.enabled_for("b"));
        s.set_enabled(None, true);
        assert!(s.enabled_for("a") && s.enabled_for("b"));
    }
}
