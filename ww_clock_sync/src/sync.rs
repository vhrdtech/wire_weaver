//! [ClockSync]: one device kept on UTC, in a task of its own.

use crate::{DeviceTime, Estimator, EstimatorConfig, Fit, HostClock, Sample};
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use wire_weaver_client::Commander;
use ww_timebase::{SyncSource, SyncStatus};

/// Client of `ww_timebase::Clock` wherever the device implements it (a global trait path, API-20).
pub struct ClockClient {
    cmd: Commander,
}

impl ClockClient {
    /// On the connection `cmd` belongs to; the clone keeps it open.
    pub fn new(cmd: &Commander) -> Self {
        ClockClient { cmd: cmd.clone() }
    }
}

mod clock_client {
    use super::*;
    wire_weaver::ww_codegen!(
        ww_timebase::Clock for ClockClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
    );
}

/// How [ClockSync] polls and steers.
#[derive(Copy, Clone, Debug)]
pub struct Options {
    /// Time between two `now()` readings.
    pub poll: Duration,
    /// Time between two steer points, once there is a fit.
    pub steer_every: Duration,
    /// Time between two checks of the device's shared time (`shared_now`, `sync_status`): the residual offset in
    /// [Status]. `None`: never.
    pub check_every: Option<Duration>,
    /// The fit's window and filter.
    pub estimator: EstimatorConfig,
}

impl Default for Options {
    /// `now()` five times a second, a steer point every second, a check every 5 s, [EstimatorConfig::default].
    fn default() -> Self {
        Options {
            poll: Duration::from_millis(200),
            steer_every: Duration::from_secs(1),
            check_every: Some(Duration::from_secs(5)),
            estimator: EstimatorConfig::default(),
        }
    }
}

/// Where a [ClockSync] is, updated after every reading.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// The current fit of the device's clock over the host's monotonic clock; `None` until there are enough samples.
    pub fit: Option<Fit>,
    /// Samples in the window.
    pub samples: usize,
    /// Steer points the device took.
    pub steers: u64,
    /// The source the device answered it follows to the last steer point: `Host` when it takes them, a line when
    /// it has something better and only notes them.
    pub source: Option<SyncSource>,
    /// The device's own status at the last check.
    pub device: Option<SyncStatus>,
    /// At the last check: the device's shared time minus the host's UTC at the middle of the round trip, in ns.
    /// What the sync achieves, within half that round trip ([Status::check_rtt]).
    pub residual: Option<i64>,
    /// Round trip of the last check, in ns.
    pub check_rtt: Option<u64>,
    /// Times the device's clock went backwards (it restarted) and the fit started over.
    pub restarts: u32,
    /// The last error of a call, cleared by the next call that works.
    pub error: Option<String>,
}

/// Keeps one device's shared clock on the host's UTC: reads `Clock::now()` every [Options::poll], fits it
/// ([Estimator]) and sends a `Clock::steer` point every [Options::steer_every], until stopped or dropped.
///
/// Any process holding a connection runs one per device (the `ww sync` command, a device's own CLI or GUI): all of
/// them steer onto the same UTC, so no central daemon is needed. Stopping leaves the device in holdover on the
/// last point.
pub struct ClockSync {
    status: watch::Receiver<Status>,
    task: JoinHandle<()>,
}

impl ClockSync {
    /// Starts on the connection of `cmd`: reads `now()` once to learn the device's time format, then runs in a
    /// tokio task. Fails when the device doesn't implement `Clock` or its time is not an integer.
    pub async fn start(
        cmd: &Commander,
        options: Options,
    ) -> Result<Self, wire_weaver_client::Error> {
        let client = ClockClient::new(cmd);
        let first = client.now().call().await?;
        let device_time = DeviceTime::new(&first).map_err(|e| {
            wire_weaver_client::Error::Other(format!("device time not usable: {e}"))
        })?;
        let (tx, status) = watch::channel(Status::default());
        let task = tokio::spawn(run(client, device_time, options, tx));
        Ok(ClockSync { status, task })
    }

    /// The latest status.
    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }

    /// Status updates as they come.
    pub fn watch(&self) -> watch::Receiver<Status> {
        self.status.clone()
    }

    /// Stops polling and steering; the device goes on in holdover.
    pub fn stop(self) {}
}

impl Drop for ClockSync {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(
    mut client: ClockClient,
    mut device_time: DeviceTime,
    options: Options,
    tx: watch::Sender<Status>,
) {
    let host = HostClock::new();
    let mut estimator = Estimator::new(options.estimator);
    let mut status = Status::default();
    let mut next_steer = 0;
    let mut next_check = 0;
    let mut poll = tokio::time::interval(options.poll);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        poll.tick().await;
        let sent = host.monotonic_ns();
        let now = client.now().call().await;
        let received = host.monotonic_ns();
        match now
            .map_err(|e| e.to_string())
            .and_then(|now| device_time.to_ns(&now).map_err(|e| e.to_string()))
        {
            Ok(device) => {
                status.error = None;
                let sample = Sample {
                    sent,
                    device,
                    received,
                };
                if estimator.add(sample) == crate::Added::Restarted {
                    status.restarts += 1;
                }
            }
            Err(e) => status.error = Some(e),
        }
        status.samples = estimator.len();
        status.fit = estimator.fit();
        if let Some(fit) = status.fit
            && received >= next_steer
        {
            next_steer = received + options.steer_every.as_nanos() as u64;
            steer(&mut client, &device_time, &fit, &host, &mut status).await;
        }
        if let Some(every) = options.check_every
            && status.steers > 0
            && received >= next_check
        {
            next_check = received + every.as_nanos() as u64;
            check(&mut client, &host, &mut status).await;
        }
        if tx.send(status.clone()).is_err() {
            return;
        }
    }
}

async fn steer(
    client: &mut ClockClient,
    device_time: &DeviceTime,
    fit: &Fit,
    host: &HostClock,
    status: &mut Status,
) {
    // the point at the device's present, by the fit
    let local = fit.local_at(host.monotonic_ns() as f64).max(0.0) as u64;
    let point = match device_time.steer_point(fit, local, host.utc_offset()) {
        Ok(point) => point,
        Err(e) => {
            status.error = Some(format!("steer point: {e}"));
            return;
        }
    };
    match client.steer(point).call().await {
        Ok(source) => {
            status.steers += 1;
            status.source = Some(source);
        }
        Err(e) => status.error = Some(format!("steer: {e}")),
    }
}

async fn check(client: &mut ClockClient, host: &HostClock, status: &mut Status) {
    let sent = host.monotonic_ns();
    let shared = client.shared_now().call().await;
    let received = host.monotonic_ns();
    match shared {
        Ok(Some(us)) => {
            let mid = (sent + (received - sent) / 2) as i128 + host.utc_offset();
            status.residual = Some((us as i128 * 1000 - mid) as i64);
            status.check_rtt = Some(received - sent);
        }
        Ok(None) => status.residual = None,
        Err(e) => status.error = Some(format!("shared_now: {e}")),
    }
    match client.sync_status().call().await {
        Ok(device) => status.device = Some(device),
        Err(e) => status.error = Some(format!("sync_status: {e}")),
    }
}
