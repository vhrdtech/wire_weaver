//! Plugins: `ww NAME ARGS..` runs the program `ww-NAME ARGS..` found on `PATH`, the way `cargo` does. A plugin brings
//! a command that needs more than `ww` should carry (`ww-chunky`: updates of chunky devices, `ww dfu flash`).
//!
//! What `ww` hands the plugin: the arguments after `NAME`, and the device selection given before it as the `WW_*`
//! environment variables (`--serial X` becomes `WW_SERIAL=X`), so the plugin selects the device as `ww` does. The
//! plugin's own command line flattens [`DeviceArgs`] (the same flags, with the same environment variables), resolves
//! them with [`Selection::resolve`] and connects with `selection.client_config()`.
//!
//! Completion: `ww`'s completion lists the `ww-*` programs found on `PATH`, but runs one with the typed words only after
//! it answered the probe `ww-NAME --ww-plugin` with the line `ww-plugin 1` (what [`complete_if_requested`] does for a
//! plugin built on this module): any other program that happens to be called `ww-something` is listed and never run
//! with the words. The answer is remembered per path and modification time.

pub use crate::device::{DeviceArgs, Selection, Setting, Source};
pub use crate::untrusted::{Untrusted, escape};
use anyhow::{Result, anyhow};
use clap_complete::CompletionCandidate;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use wire_weaver_client::ClientMeta;
use wire_weaver_client::hub::Exclusive;

/// Environment variable set for a plugin asked for completions: its value is `1`, its arguments are the words after
/// its name with the one being completed last (maybe empty), it prints one candidate per line, `value`, a tab and the
/// help text.
const COMPLETE_VAR: &str = "_WW_PLUGIN_COMPLETE";

/// The only argument of the probe, and the line a plugin answers it with.
const PROBE_ARG: &str = "--ww-plugin";
const PROBE_ANSWER: &str = "ww-plugin 1";

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
    if !wire_weaver_client::hub::hub_running() {
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
                "connecting to device ({selection}), run 'ww list{}' to see connected {}: {e:#}",
                if selection.rtt().is_some() {
                    " --probes"
                } else {
                    ""
                },
                if selection.rtt().is_some() {
                    "debug probes"
                } else {
                    "devices"
                },
            )
        })?;
    selection.check_connected(&device.device_api_info().user_api_version)?;
    Ok(device)
}

/// `ww-NAME` on `PATH`.
pub(crate) fn find(name: &str) -> Option<PathBuf> {
    let file = exe_name(name);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file))
        .find(|path| is_executable(path))
}

/// Every `ww-NAME` on `PATH`, by name, the first on `PATH` winning.
pub(crate) fn list() -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = vec![];
    let Some(path) = std::env::var_os("PATH") else {
        return found;
    };
    for dir in std::env::split_paths(&path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                let name = file.strip_prefix("ww-")?;
                let name = name.strip_suffix(".exe").unwrap_or(name);
                (!name.is_empty() && is_executable(&e.path())).then(|| (name.to_string(), e.path()))
            })
            .collect();
        names.sort();
        for (name, path) in names {
            if !found.iter().any(|(n, _)| *n == name) {
                found.push((name, path));
            }
        }
    }
    found
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("ww-{name}.exe")
    } else {
        format!("ww-{name}")
    }
}

fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// The device selection given on `ww`'s command line, as the environment the plugin reads it from. What came from the
/// environment already is inherited, what is in `ww.toml` the plugin finds itself.
pub(crate) fn selection_env(selection: &Selection, no_config: bool) -> Vec<(String, String)> {
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
        env.push(("WW_NO_CONFIG".into(), "1".into()));
    }
    env
}

/// Runs `ww-NAME ARGS..` instead of this process (on Unix it replaces it), `missing` is the error when there is no
/// such program.
pub(crate) fn run(
    name: &str,
    args: &[OsString],
    env: &[(String, String)],
    missing: impl FnOnce() -> anyhow::Error,
) -> Result<std::convert::Infallible> {
    let exe = find(name).ok_or_else(missing)?;
    let mut cmd = Command::new(&exe);
    cmd.args(args).envs(env.iter().cloned());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(anyhow!("running {}: {err}", exe.display()))
    }
    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .map_err(|e| anyhow!("running {}: {e}", exe.display()))?;
        std::process::exit(status.code().unwrap_or(1))
    }
}

/// Candidates of plugin `name` for `words` (the words after its name, the one being completed last).
pub(crate) fn complete_words(name: &str, words: &[OsString]) -> Vec<CompletionCandidate> {
    let Some(exe) = find(name) else {
        return vec![];
    };
    if !is_plugin(&exe) {
        return vec![];
    }
    let Ok(out) = Command::new(exe)
        .args(words)
        .env(COMPLETE_VAR, "1")
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| match line.split_once('\t') {
            Some((value, help)) if !help.is_empty() => {
                CompletionCandidate::new(value).help(Some(help.to_string().into()))
            }
            Some((value, _)) => CompletionCandidate::new(value),
            None => CompletionCandidate::new(line),
        })
        .collect()
}

/// Whether the program at `exe` answers the probe, from the cache when it is the same file as when asked (path and
/// modification time), else by running `exe --ww-plugin` (the only argument it ever gets from a completion before
/// that).
fn is_plugin(exe: &std::path::Path) -> bool {
    let Ok(mtime) = exe.metadata().and_then(|m| m.modified()) else {
        return false;
    };
    let stamp = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let key = format!("{}\t{stamp}", exe.display());
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|dir| dir.join("ww").join("plugins"));
    let known = cache
        .as_ref()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .unwrap_or_default();
    if let Some(line) = known.lines().find(|l| l.starts_with(&key)) {
        return line.ends_with("\tyes");
    }
    let answered = Command::new(exe)
        .arg(PROBE_ARG)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == PROBE_ANSWER);
    if let Some(file) = cache {
        let _ = std::fs::create_dir_all(file.parent().unwrap_or(&file));
        let line = format!("{key}\t{}\n", if answered { "yes" } else { "no" });
        // lines of a program that changed stay behind, the file is small (one line per version of each ww-* program)
        let _ = std::fs::write(&file, known + &line);
    }
    answered
}
