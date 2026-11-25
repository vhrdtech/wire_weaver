//! What an executable plugin (`ww-NAME` on `PATH`) needs from `ww`: the completion probe and request it answers
//! ([`complete_if_requested`]), the device selection `ww` hands it as `WW_*` environment variables
//! ([`selection_env`]), and connecting with that selection ([`connect`], [`take_exclusive`]). The host side (finding
//! and running plugins, asking them for completions) is `wire_weaver_cli::plugin`.

use crate::device::{Selection, Source};
use anyhow::{Result, anyhow};
use std::ffi::OsString;
use wire_weaver_client::ClientMeta;
use wire_weaver_client::hub::Exclusive;

/// Environment variable set for a plugin asked for completions: its value is `1`, its arguments are the words after
/// its name with the one being completed last (maybe empty), it prints one candidate per line, `value`, a tab and the
/// help text.
pub const COMPLETE_VAR: &str = "_WW_PLUGIN_COMPLETE";

/// The only argument of the probe, and the line a plugin answers it with.
pub const PROBE_ARG: &str = "--ww-plugin";
pub const PROBE_ANSWER: &str = "ww-plugin 1";

/// Plugin side: answers a completion request of `ww` and exits if there is one, otherwise does nothing. Call it
/// first thing in `main`, with the factory that builds the plugin's `clap::Command`. It also answers the probe
/// `ww-NAME --ww-plugin` that tells `ww` this is a plugin.
pub fn complete_if_requested(factory: fn() -> clap::Command) {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == PROBE_ARG) && args.next().is_none() {
        println!("{PROBE_ANSWER}");
        std::process::exit(0);
    }
    if std::env::var_os(COMPLETE_VAR).is_none() {
        return;
    }
    let words: Vec<OsString> = std::env::args_os().skip(1).collect();
    let index = words.len().saturating_sub(1);
    let mut args = vec![OsString::from("ww")];
    args.extend(words);
    let mut cmd = factory();
    let candidates =
        clap_complete::engine::complete(&mut cmd, args, index + 1, None).unwrap_or_default();
    for c in candidates {
        let help = c.get_help().map(|h| h.to_string()).unwrap_or_default();
        println!("{}\t{help}", c.get_value().to_string_lossy());
    }
    std::process::exit(0);
}

/// The whole device for a command that changes it, when a ww hub of this user serves it to other programs too (the hub
/// refuses other programs' calls and writes meanwhile and doesn't steer its clock). `None` when no hub holds it:
/// the program then holds it directly. `meta` is what the device was connected with; the claim is this client
/// instance's, and holds across a reset of the device.
pub async fn take_exclusive(
    selection: &Selection,
    meta: &ClientMeta,
    purpose: &str,
) -> Result<Option<Exclusive>> {
    // a hub serves USB and RTT devices, never a WebSocket address
    if selection.websocket().is_some() || !wire_weaver_client::hub::hub_running() {
        return Ok(None);
    }
    let key = match selection.rtt() {
        // the connect before found the probe already
        Some(_) => selection.rtt_key()?,
        None => {
            let config = selection.client_config();
            let devices = wire_weaver_client::list_usb_devices()
                .await
                .unwrap_or_default();
            let mut keys = devices
                .iter()
                .filter(|d| config.matches(d))
                .filter_map(wire_weaver_client::hub::key_of);
            match (keys.next(), keys.next()) {
                (Some(key), None) => Some(key),
                // several match (connect took one) or none on USB: nothing to tell the hub
                _ => None,
            }
        }
    };
    let Some(key) = key else {
        return Ok(None);
    };
    let exclusive = Exclusive::take(&key, meta, purpose).await?;
    if exclusive.is_some() {
        eprintln!(
            "{} {key} to this command alone while it runs (other programs on the ww hub wait)",
            console::style("exclusive").bold()
        );
    }
    Ok(exclusive)
}

/// Connects the way `ww` does for a command that talks to the device's `ww_dfu` traits whatever its API: the
/// selection (USB filters, or a debug probe with `--rtt`), as the client instance `meta`, no user API check; over RTT
/// the `api` setting is checked on the connected device. The error names `ww list` to see what there is.
pub async fn connect(
    selection: &Selection,
    meta: &ClientMeta,
) -> Result<wire_weaver_client::DynClient> {
    let config = selection
        .client_config()
        .traits_only()
        .client_meta(meta.clone());
    let device = wire_weaver_client::DynClient::from_config(config)
        .connect()
        .await
        .map_err(|e| {
            anyhow!(
                "connecting to device ({selection}){}: {e:#}",
                selection.list_hint(),
            )
        })?;
    selection.check_connected(&device.device_api_info().user_api_version)?;
    Ok(device)
}

/// The device selection given on `ww`'s command line, as the environment the plugin reads it from. What came from the
/// environment already is inherited, what is in `ww.toml` the plugin finds itself.
pub fn selection_env(selection: &Selection, no_config: bool) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = selection
        .settings
        .iter()
        .filter(|s| matches!(s.source, Source::Flag))
        .map(|s| (crate::device::env_var(s.key), s.value.clone()))
        .collect();
    if let Some(config) = &selection.explicit_file {
        env.push(("WW_CONFIG".into(), config.display().to_string()));
    }
    if no_config {
        // a bool flag's environment variable takes `true` or `false` only
        env.push(("WW_NO_CONFIG".into(), "true".into()));
    }
    env
}
