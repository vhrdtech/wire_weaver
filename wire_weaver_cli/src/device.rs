//! Device selection shared by all commands working with a device.
//!
//! Each setting is taken from the first source that has it: command line flag, environment variable,
//! project `ww.toml` (current directory or its closest parent), otherwise it is not used.
//! Settings of different kinds are combined, e.g. `api` from ww.toml and `--serial` narrow the selection together.

use crate::complete;
use anyhow::{Context, Result, anyhow, bail};
use clap::parser::ValueSource;
use clap::{ArgMatches, Args, ValueHint};
use clap_complete::ArgValueCandidates;
use semver::VersionReq;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::time::Duration;
use wire_weaver_client::ClientConfig;

pub(crate) const CONFIG_FILE: &str = "ww.toml";
/// Table in ww.toml holding device selection settings.
pub(crate) const DEVICE_TABLE: &str = "device";

/// Setting names, same for command line flags (with '-' instead of '_'), ww.toml keys and environment variables
/// (upper case with `WW_` prefix).
pub(crate) const KEYS: [&str; 8] = [
    "serial",
    "label",
    "product",
    "manufacturer",
    "api",
    "vid_pid",
    "usb_path",
    "timeout_ms",
];

#[derive(Args)]
#[command(next_help_heading = "Device selection (flags > env variables > ww.toml)")]
pub(crate) struct DeviceArgs {
    /// Serial number or its part (case-insensitive)
    #[arg(short, long, env = "WW_SERIAL", global = true, add = ArgValueCandidates::new(complete::serials))]
    serial: Option<String>,

    /// User label (case-insensitive)
    #[arg(short, long, env = "WW_LABEL", global = true, add = ArgValueCandidates::new(complete::labels))]
    label: Option<String>,

    /// Product description containing this substring (case-insensitive)
    #[arg(short, long, env = "WW_PRODUCT", global = true, add = ArgValueCandidates::new(complete::products))]
    product: Option<String>,

    /// Manufacturer containing this substring (case-insensitive)
    #[arg(long, env = "WW_MANUFACTURER", global = true, add = ArgValueCandidates::new(complete::manufacturers))]
    manufacturer: Option<String>,

    /// Implemented API, optionally with a version requirement: name or name@req, e.g. blinky_api@^0.1
    #[arg(long, env = "WW_API", global = true, add = ArgValueCandidates::new(complete::apis), value_name = "NAME[@REQ]")]
    api: Option<String>,

    /// USB vendor and product id in hex, e.g. c0de:cafe
    #[arg(long, env = "WW_VID_PID", global = true, add = ArgValueCandidates::new(complete::vid_pids), value_name = "VID:PID")]
    vid_pid: Option<String>,

    /// USB bus and port chain as shown by 'ww list', e.g. 3-1.2
    #[arg(long, env = "WW_USB_PATH", global = true, add = ArgValueCandidates::new(complete::usb_paths), value_name = "BUS-PORTS")]
    usb_path: Option<String>,

    /// Request timeout in milliseconds
    #[arg(long, env = "WW_TIMEOUT_MS", global = true, value_name = "MS")]
    timeout_ms: Option<String>,

    /// Project config file to use instead of ww.toml in the current directory or its closest parent
    #[arg(long, env = "WW_CONFIG", global = true, value_name = "PATH", value_hint = ValueHint::FilePath)]
    config: Option<PathBuf>,

    /// Do not load ww.toml
    #[arg(long, global = true)]
    no_config: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Source {
    Flag,
    Env,
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub(crate) struct Setting {
    pub(crate) key: &'static str,
    pub(crate) value: String,
    pub(crate) source: Source,
}

/// Resolved device selection.
pub(crate) struct Selection {
    pub(crate) settings: Vec<Setting>,
    /// Project config file, if one was found and loaded.
    pub(crate) file: Option<PathBuf>,
    /// Where `--config`/`WW_CONFIG` points to, even if it does not exist yet.
    pub(crate) explicit_file: Option<PathBuf>,
    client_config: ClientConfig,
}

impl Selection {
    /// `allow_missing_file` lets `--config` point to a file that doesn't exist yet (to save into it).
    pub(crate) fn resolve(matches: &ArgMatches, allow_missing_file: bool) -> Result<Self> {
        let explicit_file = matches.get_one::<PathBuf>("config").cloned();
        let file = if matches.get_flag("no_config") {
            None
        } else if let Some(path) = &explicit_file {
            if path.is_file() {
                Some(path.clone())
            } else if allow_missing_file {
                None
            } else {
                bail!("config file '{}' not found", path.display());
            }
        } else {
            find_config_file()?
        };
        let file_table = file.as_deref().map(load_device_table).transpose()?;

        let mut settings = vec![];
        for key in KEYS {
            let from_args = matches.get_one::<String>(key).and_then(|value| {
                let source = match matches.value_source(key)? {
                    ValueSource::CommandLine => Source::Flag,
                    ValueSource::EnvVariable => Source::Env,
                    _ => return None,
                };
                Some((value.clone(), source))
            });
            let from_file = file_table.as_ref().and_then(|t| t.get(key)).map(|value| {
                (
                    value.clone(),
                    Source::File(file.clone().expect("table is only loaded from a file")),
                )
            });
            if let Some((value, source)) = from_args.or(from_file) {
                settings.push(Setting { key, value, source });
            }
        }

        let mut client_config = ClientConfig::new().usb();
        for setting in &settings {
            client_config =
                apply(client_config, setting.key, &setting.value).with_context(|| {
                    format!(
                        "invalid {} '{}' from {}",
                        setting.key,
                        setting.value,
                        setting.source.describe(setting.key)
                    )
                })?;
        }
        Ok(Self {
            settings,
            file,
            explicit_file,
            client_config,
        })
    }

    pub(crate) fn client_config(&self) -> ClientConfig {
        self.client_config.clone()
    }

    /// Whether any of the settings came from the project config file.
    pub(crate) fn uses_file(&self) -> bool {
        self.settings
            .iter()
            .any(|s| matches!(s.source, Source::File(_)))
    }
}

impl Display for Selection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.settings.is_empty() {
            return write!(f, "no device selection filters");
        }
        for (i, s) in self.settings.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}={} ({})", s.key, s.value, s.source.describe(s.key))?;
        }
        Ok(())
    }
}

impl Source {
    pub(crate) fn describe(&self, key: &str) -> String {
        match self {
            Source::Flag => format!("--{}", key.replace('_', "-")),
            Source::Env => env_var(key),
            Source::File(path) => path.display().to_string(),
        }
    }
}

pub(crate) fn env_var(key: &str) -> String {
    format!("WW_{}", key.to_uppercase())
}

fn apply(config: ClientConfig, key: &str, value: &str) -> Result<ClientConfig> {
    Ok(match key {
        "serial" => config.serial_contains(value.into()),
        "label" => config.user_label_eq(value.into()),
        "product" => config.product_contains(value.into()),
        "manufacturer" => config.manufacturer_contains(value.into()),
        "api" => {
            let (gid, req) = parse_api(value)?;
            config.implements_api(gid, req)
        }
        "vid_pid" => {
            let (vid, pid) = parse_vid_pid(value)?;
            config.usb_vid_pid(vid, pid)
        }
        "usb_path" => {
            let (bus_id, port_chain) = parse_usb_path(value)?;
            config.usb_port_chain(bus_id, port_chain)
        }
        "timeout_ms" => config.default_timeout(Duration::from_millis(value.parse()?)),
        _ => unreachable!("unknown device selection key {key}"),
    })
}

pub(crate) fn parse_api(s: &str) -> Result<(String, VersionReq)> {
    match s.split_once('@') {
        Some((gid, req)) => Ok((
            gid.to_string(),
            VersionReq::parse(req).context(format!("parsing version requirement '{req}'"))?,
        )),
        None => Ok((s.to_string(), VersionReq::STAR)),
    }
}

fn parse_vid_pid(s: &str) -> Result<(u16, u16)> {
    let (vid, pid) = s
        .split_once(':')
        .ok_or(anyhow!("expected VID:PID in hex, e.g. c0de:cafe"))?;
    let hex = |n: &str| u16::from_str_radix(n.trim_start_matches("0x"), 16);
    Ok((hex(vid)?, hex(pid)?))
}

fn parse_usb_path(s: &str) -> Result<(String, Vec<u8>)> {
    let (bus_id, ports) = s
        .split_once('-')
        .ok_or(anyhow!("expected BUS-PORTS, e.g. 3-1.2"))?;
    let port_chain = ports
        .split('.')
        .map(|p| p.parse::<u8>())
        .collect::<Result<Vec<_>, _>>()?;
    Ok((bus_id.to_string(), port_chain))
}

/// Look for ww.toml in the current directory and its parents.
fn find_config_file() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir()?;
    Ok(cwd
        .ancestors()
        .map(|dir| dir.join(CONFIG_FILE))
        .find(|path| path.is_file()))
}

/// Load `[device]` table, values converted to strings, same as if they were given as flags.
fn load_device_table(path: &Path) -> Result<BTreeMap<String, String>> {
    let contents = std::fs::read_to_string(path).context(format!("reading {}", path.display()))?;
    let mut table: toml::Table =
        toml::from_str(&contents).context(format!("parsing {}", path.display()))?;
    let Some(device) = table.remove(DEVICE_TABLE) else {
        return Ok(BTreeMap::new());
    };
    let toml::Value::Table(device) = device else {
        bail!("{}: '{DEVICE_TABLE}' must be a table", path.display());
    };
    let mut strings = BTreeMap::new();
    for (key, value) in device {
        if !KEYS.contains(&key.as_str()) {
            bail!(
                "{}: unknown key '{key}' in [{DEVICE_TABLE}], expected one of: {}",
                path.display(),
                KEYS.join(", ")
            );
        }
        let value = match value {
            toml::Value::String(s) => s,
            toml::Value::Integer(i) if key == "timeout_ms" => i.to_string(),
            other => bail!(
                "{}: [{DEVICE_TABLE}] {key} must be a {}, got {}",
                path.display(),
                if key == "timeout_ms" {
                    "number"
                } else {
                    "string"
                },
                other.type_str()
            ),
        };
        strings.insert(key, value);
    }
    Ok(strings)
}
