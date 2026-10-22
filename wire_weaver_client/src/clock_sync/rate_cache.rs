//! [RateCache]: each device's clock rate as this PC last measured it, so a fresh connection steers right away.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Overrides the state dir [RateCache] keeps its files in (tests, a second setup on one PC).
pub const STATE_DIR_ENV: &str = "WW_STATE_DIR";

/// A rate older than this is not used: the clock drifted with the seasons, a fresh fit is as quick.
const MAX_AGE: Duration = Duration::from_secs(90 * 24 * 3600);

/// One device's clock rate against this PC's monotonic clock, as the last [ClockSync](super::ClockSync) of it
/// measured: the slope of its fit ([Fit::slope](super::Fit::slope)). Shared by every program that steers on this
/// PC (the hub, `ww sync`, an app in direct mode): the file is per user and per device serial, written whole to a
/// temporary file and renamed into place, so a reader never sees half of it.
///
/// With it a new connection fits from its first reading ([Estimator::set_prior](super::Estimator::set_prior)) and
/// sends its first steer point a round trip after connecting, instead of after a second of readings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateCache {
    path: PathBuf,
}

/// What [RateCache] keeps.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CachedRate {
    /// Host ns per device ns.
    pub slope: f64,
    /// When it was measured, ms since the Unix epoch.
    pub saved_ms: u64,
}

impl RateCache {
    /// The cache of the device with this serial (a USB serial number, a probe serial and chip for RTT), in the
    /// user's [state_dir].
    pub fn for_device(serial: &str) -> io::Result<RateCache> {
        Ok(Self::in_dir(&state_dir()?, serial))
    }

    /// The cache of `serial` in `dir` (its `clock_rates` subdirectory).
    pub fn in_dir(dir: &Path, serial: &str) -> RateCache {
        let name: String = serial
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
                _ => '_',
            })
            .collect();
        RateCache {
            path: dir.join("clock_rates").join(format!("{name}.toml")),
        }
    }

    /// The file it lives in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The rate saved last, `None` without one, when it can't be read or is older than 90 days.
    pub fn load(&self) -> Option<CachedRate> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let (mut rate_ppb, mut saved_ms) = (None, None);
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "rate_ppb" => rate_ppb = v.trim().parse::<f64>().ok(),
                "saved_ms" => saved_ms = v.trim().parse::<u64>().ok(),
                _ => {}
            }
        }
        let rate = CachedRate {
            slope: 1.0 + rate_ppb? * 1e-9,
            saved_ms: saved_ms?,
        };
        let age = unix_ms().saturating_sub(rate.saved_ms);
        (rate.slope.is_finite() && rate.slope > 0.0 && age < MAX_AGE.as_millis() as u64)
            .then_some(rate)
    }

    /// Saves `slope` as measured now.
    pub fn store(&self, slope: f64) -> io::Result<()> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let tmp = self
            .path
            .with_extension(format!("toml.{}.tmp", std::process::id()));
        let mut file = std::fs::File::create(&tmp)?;
        // ppb with a fraction: a slope near 1 loses digits as f64 text, the difference doesn't
        write!(
            file,
            "# ww_clock_sync: this device's clock rate against this PC's (STD-49, SRV-31)\n\
             rate_ppb = {:.3}\nsaved_ms = {}\n",
            (slope - 1.0) * 1e9,
            unix_ms()
        )?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

/// Where per-user state that outlives a reboot goes: [STATE_DIR_ENV] if set, else `$XDG_STATE_HOME/ww` or
/// `~/.local/state/ww` (Linux and other Unix), `~/Library/Application Support/ww` (macOS),
/// `%LOCALAPPDATA%\ww\state` (Windows).
pub fn state_dir() -> io::Result<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = var(STATE_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }
    let missing =
        |name: &str| io::Error::new(io::ErrorKind::NotFound, format!("{name} is not set"));
    if cfg!(windows) {
        return Ok(
            PathBuf::from(var("LOCALAPPDATA").ok_or_else(|| missing("LOCALAPPDATA"))?)
                .join("ww")
                .join("state"),
        );
    }
    if cfg!(target_os = "macos") {
        let home = var("HOME").ok_or_else(|| missing("HOME"))?;
        return Ok(PathBuf::from(home).join("Library/Application Support/ww"));
    }
    if let Some(state) = var("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state).join("ww"));
    }
    let home = var("HOME").ok_or_else(|| missing("HOME"))?;
    Ok(PathBuf::from(home).join(".local/state/ww"))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_load() {
        let dir = std::env::temp_dir().join(format!("ww_rate_cache_{}", std::process::id()));
        let cache = RateCache::in_dir(&dir, "B135/0042 x");
        assert!(cache.path().ends_with("clock_rates/B135_0042_x.toml"));
        assert_eq!(cache.load(), None);
        let slope = 1.0 / 1.01;
        cache.store(slope).unwrap();
        let rate = cache.load().unwrap();
        assert!((rate.slope - slope).abs() < 1e-12, "{}", rate.slope);
        assert!(rate.saved_ms > 0);
        // a second write replaces it, no temporary file left
        cache.store(1.0 + 20e-6).unwrap();
        assert!((cache.load().unwrap().slope - (1.0 + 20e-6)).abs() < 1e-12);
        let files = std::fs::read_dir(dir.join("clock_rates")).unwrap().count();
        assert_eq!(files, 1);
        // too old, or garbage: not used
        std::fs::write(cache.path(), "rate_ppb = 5\nsaved_ms = 1000\n").unwrap();
        assert_eq!(cache.load(), None);
        std::fs::write(cache.path(), "rate_ppb = x\n").unwrap();
        assert_eq!(cache.load(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
