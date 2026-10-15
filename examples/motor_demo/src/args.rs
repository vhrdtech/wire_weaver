//! Command line values of the `motor_demo` CLI: observe modes, durations, loads.

use std::time::Duration;

use anyhow::{Context, bail};
use motor_demo_api::Load;
use wire_weaver_client::observe::{on_change, on_change_or_beat, periodic};
use ww_client_server::observe::ObserveMode;

/// `500us`, `1ms`, `2.5s`; a bare number is milliseconds.
pub fn duration(s: &str) -> anyhow::Result<Duration> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f64 = num
        .parse()
        .with_context(|| format!("not a duration: {s}"))?;
    let secs = match unit {
        "us" | "µs" => v * 1e-6,
        "ms" | "" => v * 1e-3,
        "s" => v,
        _ => bail!("unknown unit in {s}, use us, ms or s"),
    };
    Ok(Duration::from_secs_f64(secs))
}

/// `off`, `every:1ms` (or just `1ms`), `change:1ms` (on change, at most every 1 ms), `beat:1ms/100ms` (on change,
/// at least every 100 ms).
pub fn mode(s: &str) -> anyhow::Result<ObserveMode> {
    Ok(match s.split_once(':') {
        None if s == "off" => ObserveMode::Off,
        None => periodic(duration(s)?),
        Some(("every", d)) => periodic(duration(d)?),
        Some(("change", d)) => on_change(duration(d)?),
        Some(("beat", d)) => {
            let (min, max) = d
                .split_once('/')
                .with_context(|| format!("beat:MIN/MAX, not {s}"))?;
            on_change_or_beat(duration(min)?, duration(max)?)
        }
        _ => bail!("not an observe mode: {s} (off, every:1ms, change:1ms, beat:1ms/100ms)"),
    })
}

/// A mode the way [mode] reads it.
pub fn mode_str(m: &ObserveMode) -> String {
    let d = |us: u32| -> String {
        match us {
            us if us >= 1_000_000 && us % 1_000_000 == 0 => format!("{}s", us / 1_000_000),
            us if us >= 1_000 && us % 1_000 == 0 => format!("{}ms", us / 1_000),
            us => format!("{us}us"),
        }
    };
    match m {
        ObserveMode::Off => "off".into(),
        ObserveMode::OnChange { min_us } => format!("change:{}", d(min_us.0)),
        ObserveMode::Periodic { every_us } => format!("every:{}", d(every_us.0)),
        ObserveMode::OnChangeOrBeat { min_us, max_us } => {
            format!("beat:{}/{}", d(min_us.0), d(max_us.0))
        }
        ObserveMode::Unknown(_) => "?".into(),
    }
}

/// `none`, `locked`, `viscous:B`, `fan:K`, `constant:T` (SI units, see [Load]).
pub fn load(s: &str) -> anyhow::Result<Load> {
    let num = |v: &str| -> anyhow::Result<f32> {
        v.parse().with_context(|| format!("not a number: {v}"))
    };
    Ok(match s.split_once(':') {
        None if s == "none" => Load::None,
        None if s == "locked" => Load::Locked,
        Some(("viscous", v)) => Load::Viscous { b: num(v)? },
        Some(("fan", v)) => Load::Fan { k: num(v)? },
        Some(("constant", v)) => Load::Constant { torque: num(v)? },
        _ => bail!("not a load: {s} (none, locked, viscous:B, fan:K, constant:T)"),
    })
}

/// `1234` B/s as `1.2 k`.
pub fn si(v: f64) -> String {
    match v.abs() {
        a if a >= 1e6 => format!("{:.1} M", v / 1e6),
        a if a >= 1e3 => format!("{:.1} k", v / 1e3),
        _ => format!("{v:.0} "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(duration("40us").unwrap(), Duration::from_micros(40));
        assert_eq!(duration("2.5s").unwrap(), Duration::from_millis(2_500));
        assert_eq!(duration("10").unwrap(), Duration::from_millis(10));
        assert!(duration("1h").is_err());
        let ms = Duration::from_millis(1);
        assert_eq!(mode("1ms").unwrap(), periodic(ms));
        assert_eq!(mode("change:1ms").unwrap(), on_change(ms));
        assert_eq!(
            mode("beat:1ms/100ms").unwrap(),
            on_change_or_beat(ms, 100 * ms)
        );
        assert_eq!(mode("off").unwrap(), ObserveMode::Off);
        for m in ["every:40us", "change:1ms", "beat:1ms/2s", "off"] {
            assert_eq!(mode_str(&mode(m).unwrap()), m);
        }
        assert_eq!(load("fan:2e-7").unwrap(), Load::Fan { k: 2e-7 });
        assert!(load("fan").is_err());
    }
}
