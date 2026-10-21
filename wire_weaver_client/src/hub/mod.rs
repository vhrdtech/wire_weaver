//! Reaching devices through the ww hub (CLT-39, SRV-13): direct by default, the hub when it pays off.
//!
//! [HubMode] says when `connect()` goes through the hub ([ClientConfig::hub](crate::ClientConfig::hub), env
//! [MODE_ENV]): [HubMode::Auto] by default, which opens a device directly unless the hub already holds it or the
//! device was opened often lately (the open counter, [OpenCounter]). The hub is found on the user's socket, or
//! started from a hub executable ([ensure_running]) and waited for. See `docs/transport/hub.md`.
//!
//! The routing itself needs the `hub` feature; [HubMode], the settings and the names of the environment variables
//! are always there, so an app can set them whatever the features are.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

#[cfg(feature = "hub")]
mod client;
#[cfg(feature = "hub")]
mod exclusive;
#[cfg(feature = "hub")]
mod opens;
#[cfg(feature = "hub")]
mod route;
#[cfg(feature = "hub")]
mod start;

#[cfg(feature = "hub")]
pub use client::HubClient;
#[cfg(feature = "hub")]
pub use exclusive::{Exclusive, Lend, describe_claim};
#[cfg(feature = "hub")]
pub use opens::{Opens, record_open};
#[cfg(feature = "hub")]
pub(crate) use route::{Route, route};
#[cfg(feature = "hub")]
pub use start::{
    Candidate, HubUnavailable, candidates, ensure_running, hub_running, register_self_hosting,
};

/// [HubMode] for programs whose config doesn't set one: `auto`, `prefer`, `required`, `off` (or `direct`).
pub const MODE_ENV: &str = "WW_HUB";
/// The open counter: `N/T`, N opens within T seconds move a device into the hub, e.g. `3/60` (the default).
pub const OPENS_ENV: &str = "WW_HUB_OPENS";
/// The hub executable to start, instead of looking for one.
pub const EXE_ENV: &str = "WW_HUB_EXE";
/// Set on a hub executable started by a client: serve as the hub (`ww_hub::main_if_hub`).
pub const SERVE_ENV: &str = "WW_HUB_SERVE";
/// Set with [SERVE_ENV] on a hub executable started to replace an older running hub (SRV-30): it takes over from it
/// (`ww_hub::Bound::take_over`).
pub const HANDOVER_ENV: &str = "WW_HUB_HANDOVER";
/// A hub executable prints its hub version and exits when started with this argument.
pub const VERSION_ARG: &str = "--hub-version";

/// When `connect()` goes through the hub (CLT-39). Set with [ClientConfig::hub](crate::ClientConfig::hub); a config
/// that doesn't set it takes [MODE_ENV], else [HubMode::Auto].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum HubMode {
    /// Through the hub when it already holds the device or the device was opened often lately ([OpenCounter]);
    /// otherwise directly, holding the device exclusively.
    #[default]
    Auto,
    /// Through the hub, starting one if none runs; directly when none can start or it can't serve the device.
    Prefer,
    /// Through the hub, starting one if none runs; fail without one.
    Required,
    /// Never through the hub, not even past the open counter (`WW_HUB=off`).
    Direct,
}

impl FromStr for HubMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Ok(HubMode::Auto),
            "prefer" => Ok(HubMode::Prefer),
            "required" | "require" => Ok(HubMode::Required),
            "off" | "direct" | "no" | "0" => Ok(HubMode::Direct),
            other => Err(format!(
                "{MODE_ENV}={other}: expected auto, prefer, required or off"
            )),
        }
    }
}

impl fmt::Display for HubMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HubMode::Auto => "auto",
            HubMode::Prefer => "prefer",
            HubMode::Required => "required",
            HubMode::Direct => "off",
        })
    }
}

impl HubMode {
    /// [MODE_ENV], [HubMode::Auto] when not set; a value that doesn't parse is reported and taken as `Auto`.
    pub fn from_env() -> HubMode {
        match std::env::var(MODE_ENV) {
            Ok(s) => s.parse().unwrap_or_else(|e| {
                tracing::warn!("{e}, using auto");
                HubMode::Auto
            }),
            Err(_) => HubMode::Auto,
        }
    }
}

/// When repeated opens move a device into the hub ([HubMode::Auto]): `opens` within `window`. A shell-launched call
/// (client kind `Cli`) counts 1, every other kind 1/4, so a loop of `ww` calls in a terminal goes through the hub
/// from its third call while a GUI restarted twice doesn't.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OpenCounter {
    pub opens: f32,
    pub window: Duration,
}

impl Default for OpenCounter {
    /// 3 opens within 60 s.
    fn default() -> Self {
        OpenCounter {
            opens: 3.0,
            window: Duration::from_secs(60),
        }
    }
}

impl FromStr for OpenCounter {
    type Err = String;

    /// `N/T`: N opens within T seconds, e.g. `3/60`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || format!("{OPENS_ENV}={s}: expected N/T, N opens within T seconds, e.g. 3/60");
        let (n, t) = s.split_once('/').ok_or_else(err)?;
        let opens: f32 = n.trim().parse().map_err(|_| err())?;
        let secs: f32 = t.trim().parse().map_err(|_| err())?;
        if !(opens > 0.0 && secs > 0.0 && secs.is_finite()) {
            return Err(err());
        }
        Ok(OpenCounter {
            opens,
            window: Duration::from_secs_f32(secs),
        })
    }
}

impl OpenCounter {
    /// [OPENS_ENV], the default when not set; a value that doesn't parse is reported and the default taken.
    pub fn from_env() -> OpenCounter {
        match std::env::var(OPENS_ENV) {
            Ok(s) => s.parse().unwrap_or_else(|e| {
                tracing::warn!("{e}, using 3/60");
                OpenCounter::default()
            }),
            Err(_) => OpenCounter::default(),
        }
    }
}

/// The key the hub's registry knows `device` by (`usb:<vid>:<pid>:<serial>`), `None` for a device not on USB.
pub fn key_of(device: &crate::DeviceInfo) -> Option<String> {
    let usb = device.usb.as_ref()?;
    Some(crate::holders::key_for(crate::holders::DeviceRef::Usb {
        vid: usb.vid,
        pid: usb.pid,
        serial: device.serials.first().map(String::as_str).unwrap_or(""),
        iface: None,
    }))
}

/// `ww hub take`: the hub opens the device `key` now and keeps it, so programs reach it through the hub from their
/// first call. Starts the hub if none runs.
#[cfg(feature = "hub")]
pub async fn take(key: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let socket = ensure_running(true).await?;
    let hub = HubClient::config(|c| c.hub_api_at(socket.display().to_string()))
        .connect()
        .await
        .context("connecting to the ww hub")?;
    match hub.registry().take(key.to_string()).call().await {
        Ok(()) => Ok(()),
        Err(e) if e.is_unimplemented() => anyhow::bail!(
            "the running ww hub can't take devices yet (it doesn't open devices, SRV-29)"
        ),
        Err(crate::CallError::Device(e)) => anyhow::bail!("the ww hub can't take {key}: {e:?}"),
        Err(e) => anyhow::bail!("ww hub take {key}: {e:?}"),
    }
}

/// The end of a busy error: how the device could be shared instead, from who holds it.
#[cfg_attr(not(any(feature = "usb", feature = "rtt")), allow(dead_code))]
pub(crate) fn busy_hint(holder: &crate::holders::Holder) -> String {
    match holder.mode {
        crate::holders::Mode::Hub if cfg!(feature = "hub") => format!(
            "; the ww hub holds it, reach it through the hub ({MODE_ENV} unset, auto, prefer or required, not off)"
        ),
        crate::holders::Mode::Hub => "; the ww hub holds it, reach it through the hub: build with \
                                      wire_weaver_client's `hub` feature"
            .to_string(),
        crate::holders::Mode::Direct => format!(
            "; to share a device between programs, let the ww hub hold it: `ww hub take`, or {MODE_ENV}=prefer \
             for both programs (a hub comes with `ww`: cargo install wire_weaver_cli)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse() {
        assert_eq!("Auto".parse(), Ok(HubMode::Auto));
        assert_eq!("prefer".parse(), Ok(HubMode::Prefer));
        assert_eq!("required".parse(), Ok(HubMode::Required));
        assert_eq!("off".parse(), Ok(HubMode::Direct));
        assert_eq!("direct".parse(), Ok(HubMode::Direct));
        assert!("sometimes".parse::<HubMode>().is_err());
        for mode in [
            HubMode::Auto,
            HubMode::Prefer,
            HubMode::Required,
            HubMode::Direct,
        ] {
            assert_eq!(mode.to_string().parse(), Ok(mode));
        }
    }

    #[test]
    fn open_counter_parses() {
        let c: OpenCounter = "5/30".parse().unwrap();
        assert_eq!((c.opens, c.window), (5.0, Duration::from_secs(30)));
        assert!("5".parse::<OpenCounter>().is_err());
        assert!("0/30".parse::<OpenCounter>().is_err());
        assert!("3/-1".parse::<OpenCounter>().is_err());
    }
}
