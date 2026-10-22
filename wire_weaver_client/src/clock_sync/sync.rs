//! [ClockSync]: one device kept on UTC, in a task of its own.

use super::{DeviceTime, Estimator, EstimatorConfig, Fit, HostClock, RateCache, Sample};
use crate::Commander;
use crate::event_loop::commander::WeakCommander;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use ww_timebase::{SteererOwned, SyncSource, SyncState, SyncStatusOwned};

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

/// How [ClockSync] polls and steers: at [Options::acquire] while the device is getting there, at the normal pace
/// once it says it is `Locked`.
#[derive(Clone, Debug)]
pub struct Options {
    /// Time between two `now()` readings, once locked.
    pub poll: Duration,
    /// Time between two steer points, once locked (and there is a fit).
    pub steer_every: Duration,
    /// Time between two checks of the device's shared time (`shared_now`, `sync_status`), once locked: the
    /// residual offset and the device's state in [Status]. `None`: never, and the device's state is never known,
    /// so it stays at the acquiring pace.
    pub check_every: Option<Duration>,
    /// The pace while the device isn't `Locked` yet (`Free`, `Acquiring`, `Holdover`, or not checked yet): more
    /// steer points, so the device follows the young fit closely and is within its tolerance sooner.
    pub acquire: Pace,
    /// The fit's window and filter.
    pub estimator: EstimatorConfig,
    /// Who the steer points say they are from (SRV-31); `None`: this program, from
    /// [ClientMeta](crate::ClientMeta)'s defaults with an id of its own ([steerer]).
    pub steerer: Option<SteererOwned>,
    /// Where the device's rate is kept between connections: read at the start (the first steer point goes out after
    /// the first reading), written every [Options::save_rate_every] once the fit has its own rate. `None`: none.
    pub rate_cache: Option<RateCache>,
    /// See [Options::rate_cache].
    pub save_rate_every: Duration,
}

/// Poll, steer and check intervals of one phase, see [Options].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pace {
    pub poll: Duration,
    pub steer_every: Duration,
    pub check_every: Option<Duration>,
}

impl Default for Options {
    /// Once locked `now()` five times a second, a steer point every second, a check every 5 s; while acquiring all
    /// three ten times as often, and a check every second; [EstimatorConfig::default].
    fn default() -> Self {
        Options {
            poll: Duration::from_millis(200),
            steer_every: Duration::from_secs(1),
            check_every: Some(Duration::from_secs(5)),
            acquire: Pace {
                poll: Duration::from_millis(100),
                steer_every: Duration::from_millis(100),
                check_every: Some(Duration::from_secs(1)),
            },
            estimator: EstimatorConfig::default(),
            steerer: None,
            rate_cache: None,
            save_rate_every: Duration::from_secs(60),
        }
    }
}

/// This program as a steerer: program, host and pid as [ClientMeta](crate::ClientMeta) finds them, and
/// the id of `meta` (one per client object), or of a new one.
pub fn steerer(meta: Option<&crate::ClientMeta>) -> SteererOwned {
    let meta = meta.cloned().unwrap_or_default();
    let resolved = meta.resolve();
    SteererOwned {
        id: meta.instance_id(),
        host: resolved.host,
        program: resolved.program,
        pid: resolved.pid,
    }
}

impl Options {
    /// The pace for a device in `state` (as its last check said, `None` before one): normal once `Locked`, the
    /// acquiring one otherwise.
    pub fn pace(&self, state: Option<SyncState>) -> Pace {
        match state {
            Some(SyncState::Locked) => Pace {
                poll: self.poll,
                steer_every: self.steer_every,
                check_every: self.check_every,
            },
            _ => self.acquire,
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
    /// Steer points the device answered, followed or only noted.
    pub steers: u64,
    /// The source the device answered it follows to the last steer point: `Host` when it takes them, a line when
    /// it has something better and only notes them, `None` when it follows nothing (its source list leaves `Host`
    /// out and nothing else is locked).
    pub source: Option<SyncSource>,
    /// The device answered a steer point: `source` is its answer, not just unknown yet.
    pub answered: bool,
    /// The device's own status at the last check.
    pub device: Option<SyncStatusOwned>,
    /// Whether the device follows this sync's points, by its last check (SRV-31): `Some(false)` when it follows
    /// another steerer (`device.steerer` says who) and only notes ours; `None` until checked, or from a device that
    /// doesn't say whom it follows.
    pub followed: Option<bool>,
    /// The fit started from the [RateCache]'s rate.
    pub rate_from_cache: bool,
    /// Steering is paused ([ClockSync::set_paused]).
    pub paused: bool,
    /// Polling and steering at [Options::acquire]: the device didn't say `Locked` at the last check.
    pub acquiring: bool,
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
    paused: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ClockSync {
    /// Starts on the connection of `cmd`: reads `now()` once to learn the device's time format, then runs in a
    /// tokio task. Fails when the device doesn't implement `Clock` or its time is not an integer.
    ///
    /// The sync keeps a clone of `cmd`, so the connection stays open while it runs (a connection's own steering,
    /// [Commander::clock_sync], doesn't).
    pub async fn start(cmd: &Commander, options: Options) -> Result<Self, crate::Error> {
        let device_time = first_reading(cmd).await?;
        let mut options = options;
        let steerer = options.steerer.take().unwrap_or_else(|| steerer(None));
        let (tx, status) = watch::channel(Status::default());
        let (paused, paused_rx) = watch::channel(false);
        let conn = Conn::Strong(cmd.clone());
        let task =
            tokio::spawn(
                async move { run(conn, device_time, options, steerer, paused_rx, &tx).await },
            );
        Ok(ClockSync {
            status,
            paused,
            task,
        })
    }

    /// Pauses steering, or goes on: while paused the device's clock is still read (the fit stays current), but no
    /// steer point is sent, so the device coasts in holdover (the hub pauses while a program has the whole device to
    /// itself, SRV-32).
    pub fn set_paused(&self, paused: bool) {
        self.paused.send_replace(paused);
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

/// The device's time format, from a first `now()` reading.
pub(crate) async fn first_reading(cmd: &Commander) -> Result<DeviceTime, crate::Error> {
    let client = ClockClient::new(cmd);
    let first = client.now().call().await?;
    DeviceTime::new(&first).map_err(|e| crate::Error::Other(format!("device time not usable: {e}")))
}

/// The connection a sync's calls go through.
pub(crate) enum Conn {
    /// Kept open by the sync ([ClockSync::start])
    Strong(Commander),
    /// Not kept open: the sync ends when the program's last Commander of it is gone (CLT-46)
    Weak(WeakCommander),
}

impl Conn {
    /// A Commander for the next calls, dropped again before the sync waits; `None` once the connection is gone.
    fn get(&self) -> Option<Commander> {
        match self {
            Conn::Strong(cmd) => Some(cmd.clone()),
            Conn::Weak(weak) => weak.upgrade(),
        }
    }
}

/// Polls, steers and checks until the connection or the status receivers are gone.
pub(crate) async fn run(
    conn: Conn,
    mut device_time: DeviceTime,
    options: Options,
    steerer: SteererOwned,
    paused: watch::Receiver<bool>,
    tx: &watch::Sender<Status>,
) {
    let host = HostClock::new();
    let mut estimator = Estimator::new(options.estimator);
    let mut status = Status::default();
    if let Some(rate) = options.rate_cache.as_ref().and_then(RateCache::load) {
        estimator.set_prior(Some(rate.slope));
        status.rate_from_cache = true;
    }
    let mut next_save = options.save_rate_every.as_nanos() as u64;
    let mut next_steer = 0;
    let mut next_check = 0;
    let mut next_poll = tokio::time::Instant::now();
    loop {
        let pace = options.pace(status.device.as_ref().map(|d| d.state));
        status.acquiring = pace != options.pace(Some(SyncState::Locked));
        tokio::time::sleep_until(next_poll).await;
        // a slow answer delays the next reading rather than bunching them up
        next_poll = (next_poll + pace.poll).max(tokio::time::Instant::now());
        // held for this round's calls only
        let Some(cmd) = conn.get() else {
            return;
        };
        let mut client = ClockClient { cmd };
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
                if estimator.add(sample) == super::Added::Restarted {
                    status.restarts += 1;
                }
            }
            Err(e) => status.error = Some(e),
        }
        status.samples = estimator.len();
        status.fit = estimator.fit();
        status.paused = *paused.borrow();
        if let Some(fit) = status.fit
            && !status.paused
            && received >= next_steer
        {
            next_steer = received + pace.steer_every.as_nanos() as u64;
            steer(
                &mut client,
                &device_time,
                &fit,
                &steerer,
                &host,
                &mut status,
            )
            .await;
        }
        if let (Some(cache), Some(fit)) = (&options.rate_cache, status.fit)
            && !fit.prior
            && estimator.span() >= options.estimator.prior_span.as_nanos() as u64
            && received >= next_save
        {
            next_save = received + options.save_rate_every.as_nanos() as u64;
            if let Err(e) = cache.store(fit.slope) {
                status.error = Some(format!(
                    "saving the rate to {}: {e}",
                    cache.path().display()
                ));
            }
        }
        if let Some(every) = pace.check_every
            && status.steers > 0
            && received >= next_check
        {
            next_check = received + every.as_nanos() as u64;
            check(&mut client, &host, &mut status).await;
            status.followed = status
                .device
                .as_ref()
                .and_then(|d| d.steerer.as_ref())
                .map(|s| s.id == steerer.id);
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
    steerer: &SteererOwned,
    host: &HostClock,
    status: &mut Status,
) {
    // the point at the device's present, by the fit
    let local = fit.local_at(host.monotonic_ns() as f64).max(0.0) as u64;
    let point = match device_time.steer_point(fit, local, host.utc_offset()) {
        Ok(point) => ww_timebase::SteerPointOwned {
            steerer: Some(steerer.clone()),
            ..point
        },
        Err(e) => {
            status.error = Some(format!("steer point: {e}"));
            return;
        }
    };
    match client.steer(point).call().await {
        Ok(source) => {
            status.steers += 1;
            status.source = source;
            status.answered = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_until_locked() {
        let o = Options::default();
        let normal = o.pace(Some(SyncState::Locked));
        assert_eq!(normal.steer_every, Duration::from_secs(1));
        for state in [
            None,
            Some(SyncState::Free),
            Some(SyncState::Acquiring),
            Some(SyncState::Holdover),
        ] {
            let p = o.pace(state);
            assert_eq!(p, o.acquire, "{state:?}");
            assert!(p.steer_every < normal.steer_every && p.poll < normal.poll);
        }
    }
}
