//! Observing the demo from the host: one observer per level, every update counted in [Stats].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tokio::task::JoinHandle;
use wire_weaver_client::observe::{LevelConfig, LevelUpdate, PreparedObserve};
use ww_client_server::observe::ObserveMode;

use crate::client::MotorDemo;
use crate::client::api_client::motor_demo_api_motor_demo::{
    motor_demo_api_control::ControlObserve, motor_demo_api_foc::FocObserve,
    motor_demo_api_telemetry::TelemetryObserve,
};
use crate::stats::{Carried, Level, Stats, control_present, foc_present, telemetry_present};

/// What to ask of one level.
#[derive(Clone, Debug)]
pub struct LevelRequest {
    pub level: Level,
    /// Mode per property, by resource index.
    pub modes: BTreeMap<u32, ObserveMode>,
    /// Bytes per second of device time, `None` leaves it to the device.
    pub budget: Option<u32>,
}

/// What the device applies to a level.
#[derive(Clone, Debug)]
pub struct LevelGranted {
    pub modes: BTreeMap<u32, ObserveMode>,
    /// Bytes per second of device time.
    pub budget: u32,
    pub refused: usize,
}

/// Levels being observed, counted into `stats`. Dropping it stops observing.
pub struct Session {
    pub stats: Arc<Mutex<Stats>>,
    pub granted: BTreeMap<Level, LevelGranted>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl Session {
    /// The counts so far, starting over.
    pub fn take(&self) -> Stats {
        self.stats.lock().unwrap().take()
    }
}

/// Observe the levels of `requests` (levels without properties are skipped).
pub async fn observe(client: &MotorDemo, requests: &[LevelRequest]) -> anyhow::Result<Session> {
    let stats = Arc::new(Mutex::new(Stats::new()));
    let mut session = Session {
        stats: stats.clone(),
        granted: BTreeMap::new(),
        tasks: vec![],
    };
    for r in requests.iter().filter(|r| !r.modes.is_empty()) {
        let (granted, task) = match r.level {
            Level::Foc => {
                let p = client.foc().observe(FocObserve::from_entries(&r.modes));
                start(p, r, stats.clone(), foc_present).await?
            }
            Level::Control => {
                let p = client
                    .control()
                    .observe(ControlObserve::from_entries(&r.modes));
                start(p, r, stats.clone(), control_present).await?
            }
            Level::Telemetry => {
                let p = client
                    .telemetry()
                    .observe(TelemetryObserve::from_entries(&r.modes));
                start(p, r, stats.clone(), telemetry_present).await?
            }
        };
        session.granted.insert(r.level, granted);
        session.tasks.push(task);
    }
    Ok(session)
}

async fn start<U: LevelUpdate>(
    prepared: PreparedObserve<U, U>,
    r: &LevelRequest,
    stats: Arc<Mutex<Stats>>,
    carried: fn(&U) -> Carried,
) -> anyhow::Result<(LevelGranted, JoinHandle<()>)> {
    let prepared = match r.budget {
        Some(b) => prepared.budget(b),
        None => prepared,
    };
    // a fast level fills the default queue in no time when the host falls behind
    let mut observer = prepared.queue_bytes(4 << 20).subscribe().await?;
    let g = observer
        .granted()
        .ok_or_else(|| anyhow::anyhow!("{}: no answer to observe", r.level.name()))?;
    let granted = LevelGranted {
        modes: g.modes.to_entries(),
        budget: g.budget,
        refused: g.refused.len(),
    };
    let level = r.level;
    let task = tokio::spawn(async move {
        while let Ok(o) = observer.recv().await {
            stats.lock().unwrap().record_observed(level, &o, carried);
        }
    });
    Ok((granted, task))
}
