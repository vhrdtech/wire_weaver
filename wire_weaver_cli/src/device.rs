//! Device selection shared by all commands working with a device.
//!
//! Each setting is taken from the first source that has it: command line flag, environment variable,
//! project `ww.toml` (current directory or its closest parent), otherwise it is not used.
//! Settings of different kinds are combined, e.g. `api` from ww.toml and `--serial` narrow the selection together.
//!
//! `rtt` connects through a debug probe instead of USB (TOOL-20): `probe` and `probe_speed_khz` go with it, `elf` too,
//! the USB filters (`serial`, `label`, ..) don't, and `api` is checked once connected.

use crate::complete;
use anyhow::{Context, Result, anyhow, bail};
use clap::parser::ValueSource;
use clap::{ArgMatches, Args, ValueHint};
use clap_complete::{ArgValueCandidates, ArgValueCompleter};
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
pub(crate) const KEYS: [&str; 13] = [
    "serial",
    "label",
    "product",
    "manufacturer",
    "api",
    "vid_pid",
    "usb_path",
    "rtt",
    "probe",
    "elf",
    "probe_speed_khz",
    "timeout_ms",
    "link_setup_timeout_ms",
];

/// Settings holding a number (integers in ww.toml).
pub(crate) const NUMERIC_KEYS: [&str; 3] =
    ["timeout_ms", "link_setup_timeout_ms", "probe_speed_khz"];

/// Filters describing a USB device, which a device behind a debug probe doesn't have.
const USB_KEYS: [&str; 6] = [
    "serial",
    "label",
    "product",
    "manufacturer",
    "vid_pid",
    "usb_path",
];

/// Settings that only mean something with `rtt` (`elf` also names the ELF `counters` and `report` decode with).
const PROBE_KEYS: [&str; 2] = ["probe", "probe_speed_khz"];

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

    /// Connect over RTT through a debug probe instead of USB: the probe-rs chip name, e.g. STM32G0B1RETx
    #[arg(long, env = "WW_RTT", global = true, value_name = "CHIP", add = ArgValueCompleter::new(complete::rtt_chips))]
    rtt: Option<String>,

    /// Debug probe to use with --rtt, when there are several: its serial number or a part of it, VID:PID in hex or
    /// VID:PID:SERIAL
    #[arg(long, env = "WW_PROBE", global = true, value_name = "SERIAL|VID:PID", add = ArgValueCandidates::new(complete::probes))]
    probe: Option<String>,

    /// Firmware ELF file: with --rtt the RTT control block address is taken from it instead of scanning RAM (~1.5 s on
    /// an ST-LINK; RAM is still scanned if the running firmware doesn't match it); `counters` and `report` decode
    /// with it when given here on the command line
    #[arg(long, env = "WW_ELF", global = true, value_name = "ELF", value_hint = ValueHint::FilePath)]
    elf: Option<String>,

    /// SWD clock with --rtt in kHz (the probe's default if not given)
    #[arg(long, env = "WW_PROBE_SPEED_KHZ", global = true, value_name = "KHZ")]
    probe_speed_khz: Option<String>,

    /// Request timeout in milliseconds
    #[arg(long, env = "WW_TIMEOUT_MS", global = true, value_name = "MS")]
    timeout_ms: Option<String>,

    /// How long to wait for the device to answer when connecting, in milliseconds (default depends on the transport:
    /// 1000 for USB), raise it if connecting fails with "LinkSetup was not received"
    #[arg(
        long,
        env = "WW_LINK_SETUP_TIMEOUT_MS",
        global = true,
        value_name = "MS"
    )]
    link_setup_timeout_ms: Option<String>,

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
    /// USB filters left out because `rtt` from a source that comes first selects a debug probe.
    pub(crate) unused: Vec<Setting>,
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

        let (used, unused) = split_by_transport(&settings)?;
        let client_config = client_config(&used)?;
        Ok(Self {
            settings,
            unused,
            file,
            explicit_file,
            client_config,
        })
    }

    pub(crate) fn client_config(&self) -> ClientConfig {
        self.client_config.clone()
    }

    /// `--elf` from the command line: the ELF `counters` and `report` decode with instead of looking for the one
    /// with the device's build id. From an env variable or ww.toml it's only the RTT control block's.
    pub(crate) fn elf_flag(&self) -> Option<PathBuf> {
        self.settings
            .iter()
            .find(|s| s.key == "elf" && matches!(s.source, Source::Flag))
            .map(elf_path)
    }

    /// The chip, when the device is reached over RTT through a debug probe.
    pub(crate) fn rtt(&self) -> Option<&str> {
        self.get("rtt")
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.value.as_str())
    }

    /// The single debug probe `--probe` picks for `--rtt` (any one, when it doesn't narrow down to one).
    pub(crate) fn rtt_probe(&self) -> Result<wire_weaver_client::ProbeInfo> {
        let probes = wire_weaver_client::list_probes();
        if probes.is_empty() {
            bail!(
                "no debug probe found, is it plugged in? (`probe-rs list` shows what probe-rs sees)"
            );
        }
        let filter = self.get("probe").map(ProbeFilter::parse).transpose()?;
        let mut matching: Vec<_> = probes
            .iter()
            .filter(|p| filter.as_ref().is_none_or(|f| f.matches(p)))
            .cloned()
            .collect();
        match matching.len() {
            1 => Ok(matching.remove(0)),
            0 => bail!(
                "no debug probe matches --probe {}, found: {}",
                self.get("probe").unwrap_or_default(),
                probe_names(&probes)
            ),
            _ => bail!(
                "several debug probes found, pick one with --probe SERIAL: {}",
                probe_names(&matching)
            ),
        }
    }

    /// The hub's and the lock file's name of the device: `rtt:<probe serial>:<chip>` over RTT, see
    /// [holders::key_for](wire_weaver_client::holders::key_for).
    pub(crate) fn rtt_key(&self) -> Result<Option<String>> {
        let Some(chip) = self.rtt() else {
            return Ok(None);
        };
        let probe = self.rtt_probe()?;
        Ok(Some(wire_weaver_client::holders::key_for(
            wire_weaver_client::holders::DeviceRef::Rtt {
                probe_serial: probe.serial.as_deref().unwrap_or_default(),
                chip,
            },
        )))
    }

    /// Over RTT the device isn't known before connecting: checks `api` on the connected device. Nothing to do over
    /// USB, where the device was picked by it.
    pub(crate) fn check_connected(
        &self,
        api: &wire_weaver::ww_version::FullVersionOwned,
    ) -> Result<()> {
        if self.rtt().is_none() {
            return Ok(());
        }
        let Some(setting) = self.settings.iter().find(|s| s.key == "api") else {
            return Ok(());
        };
        let (gid, req) = parse_api(&setting.value)?;
        let version = semver::Version::new(
            api.version.major.0 as u64,
            api.version.minor.0 as u64,
            api.version.patch.0 as u64,
        );
        if api.crate_id != gid || !req.matches(&version) {
            bail!(
                "the device behind the probe serves {} {version}, not api={} ({})",
                api.crate_id,
                setting.value,
                setting.source.describe(setting.key)
            );
        }
        Ok(())
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
        let used = self
            .settings
            .iter()
            .filter(|s| !self.unused.iter().any(|u| u.key == s.key));
        for (i, s) in used.enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}={} ({})", s.key, s.value, s.source.describe(s.key))?;
        }
        if !self.unused.is_empty() {
            let keys: Vec<_> = self.unused.iter().map(|s| s.key).collect();
            write!(f, "; not used over RTT: {}", keys.join(", "))?;
        }
        Ok(())
    }
}

impl Source {
    /// Flags win over env variables, which win over ww.toml.
    fn rank(&self) -> u8 {
        match self {
            Source::Flag => 2,
            Source::Env => 1,
            Source::File(_) => 0,
        }
    }
}

/// Settings used and left out. With `rtt`, the USB filters from a source that comes after it (ww.toml under
/// `--rtt`) are left out, the ones from the same or a stronger one are an error: they'd select a USB device.
/// Without `rtt`, a probe setting is an error.
fn split_by_transport(settings: &[Setting]) -> Result<(Vec<Setting>, Vec<Setting>)> {
    let Some(rtt) = settings.iter().find(|s| s.key == "rtt") else {
        if let Some(s) = settings.iter().find(|s| PROBE_KEYS.contains(&s.key)) {
            bail!(
                "{}={} ({}) needs rtt: pass --rtt CHIP to connect through a debug probe",
                s.key,
                s.value,
                s.source.describe(s.key)
            );
        }
        return Ok((settings.to_vec(), vec![]));
    };
    let mut used = vec![];
    let mut unused = vec![];
    for s in settings {
        if !USB_KEYS.contains(&s.key) {
            used.push(s.clone());
        } else if s.source.rank() < rtt.source.rank() {
            unused.push(s.clone());
        } else {
            bail!(
                "{}={} ({}) selects a USB device, but rtt={} ({}) connects through a debug probe: pick the probe \
                 with --probe SERIAL or VID:PID",
                s.key,
                s.value,
                s.source.describe(s.key),
                rtt.value,
                rtt.source.describe(rtt.key)
            );
        }
    }
    Ok((used, unused))
}

/// USB filters, or the device's filters, then RTT with the probe's (see [ClientConfig::rtt]).
fn client_config(settings: &[Setting]) -> Result<ClientConfig> {
    fn with_context<T>(r: Result<T>, s: &Setting) -> Result<T> {
        r.with_context(|| {
            format!(
                "invalid {} '{}' from {}",
                s.key,
                s.value,
                s.source.describe(s.key)
            )
        })
    }
    let rtt = settings.iter().find(|s| s.key == "rtt");
    let mut config = match rtt {
        Some(_) => ClientConfig::new(),
        None => ClientConfig::new().usb(),
    };
    for s in settings
        .iter()
        .filter(|s| !["rtt", "elf"].contains(&s.key) && !PROBE_KEYS.contains(&s.key))
    {
        config = with_context(apply(config, s.key, &s.value), s)?;
    }
    let Some(rtt) = rtt else {
        return Ok(config);
    };
    let speed_hz = match settings.iter().find(|s| s.key == "probe_speed_khz") {
        Some(s) => {
            let khz = with_context(
                s.value.parse::<std::num::NonZeroU32>().map_err(Into::into),
                s,
            )?;
            Some(khz.get().saturating_mul(1000))
        }
        None => None,
    };
    config = config.rtt(rtt.value.clone(), speed_hz);
    if let Some(s) = settings.iter().find(|s| s.key == "probe") {
        config = with_context(ProbeFilter::parse(&s.value), s)?.apply(config);
    }
    if let Some(s) = settings.iter().find(|s| s.key == "elf") {
        config = config.rtt_elf(elf_path(s));
    }
    Ok(config)
}

/// An ELF path from ww.toml is relative to the file's directory, from a flag or env variable to the current one.
pub(crate) fn elf_path(s: &Setting) -> PathBuf {
    let path = PathBuf::from(&s.value);
    match &s.source {
        Source::File(file) if path.is_relative() => {
            file.parent().map(|dir| dir.join(&path)).unwrap_or(path)
        }
        _ => path,
    }
}

/// `--probe`: a serial number or its part, VID:PID, or VID:PID:SERIAL.
#[derive(Debug, PartialEq)]
struct ProbeFilter {
    vid_pid: Option<(u16, u16)>,
    serial: Option<String>,
}

impl ProbeFilter {
    fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        Ok(match parts.as_slice() {
            [serial] => ProbeFilter {
                vid_pid: None,
                serial: Some(serial.to_string()),
            },
            [vid, pid] => ProbeFilter {
                vid_pid: Some(parse_vid_pid(&format!("{vid}:{pid}"))?),
                serial: None,
            },
            [vid, pid, serial] => ProbeFilter {
                vid_pid: Some(parse_vid_pid(&format!("{vid}:{pid}"))?),
                serial: Some(serial.to_string()),
            },
            _ => unreachable!("splitn gives one to three parts"),
        })
    }

    fn matches(&self, p: &wire_weaver_client::ProbeInfo) -> bool {
        let vid_pid_ok = self.vid_pid.is_none_or(|v| v == (p.vid, p.pid));
        let serial_ok = self.serial.as_ref().is_none_or(|s| {
            p.serial
                .as_ref()
                .is_some_and(|ps| ps.to_lowercase().contains(&s.to_lowercase()))
        });
        vid_pid_ok && serial_ok
    }

    /// Filters after [ClientConfig::rtt] pick the probe.
    fn apply(self, mut config: ClientConfig) -> ClientConfig {
        if let Some((vid, pid)) = self.vid_pid {
            config = config.usb_vid_pid(vid, pid);
        }
        if let Some(serial) = self.serial {
            config = config.serial_contains(serial);
        }
        config
    }
}

fn probe_names(probes: &[wire_weaver_client::ProbeInfo]) -> String {
    let names: Vec<_> = probes
        .iter()
        .map(|p| {
            format!(
                "{} {:04x}:{:04x}:{}",
                p.name,
                p.vid,
                p.pid,
                p.serial.as_deref().unwrap_or("<no serial>")
            )
        })
        .collect();
    names.join(", ")
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
        "link_setup_timeout_ms" => config.link_setup_timeout(Duration::from_millis(
            value.parse::<std::num::NonZeroU64>()?.get(),
        )),
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
            toml::Value::Integer(i) if NUMERIC_KEYS.contains(&key.as_str()) => i.to_string(),
            other => bail!(
                "{}: [{DEVICE_TABLE}] {key} must be a {}, got {}",
                path.display(),
                if NUMERIC_KEYS.contains(&key.as_str()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(key: &'static str, value: &str, source: Source) -> Setting {
        Setting {
            key,
            value: value.into(),
            source,
        }
    }

    fn file() -> Source {
        Source::File(PathBuf::from("/work/board/ww.toml"))
    }

    #[test]
    fn usb_filters_from_ww_toml_are_left_out_under_rtt() {
        let settings = [
            setting("rtt", "STM32G0B1RETx", Source::Flag),
            setting("serial", "0B1", file()),
            setting("api", "g0b1re_api", file()),
        ];
        let (used, unused) = split_by_transport(&settings).unwrap();
        let used: Vec<_> = used.iter().map(|s| s.key).collect();
        let unused: Vec<_> = unused.iter().map(|s| s.key).collect();
        assert_eq!(used, ["rtt", "api"]);
        assert_eq!(unused, ["serial"]);
    }

    #[test]
    fn usb_filters_as_strong_as_rtt_are_an_error() {
        for source in [Source::Flag, Source::Env] {
            let settings = [
                setting("rtt", "STM32G0B1RETx", Source::Env),
                setting("label", "bench", source),
            ];
            let e = split_by_transport(&settings).unwrap_err().to_string();
            assert!(e.contains("selects a USB device"), "{e}");
        }
        // both in ww.toml: the file contradicts itself
        let settings = [
            setting("rtt", "STM32G0B1RETx", file()),
            setting("vid_pid", "c0de:cafe", file()),
        ];
        assert!(split_by_transport(&settings).is_err());
    }

    #[test]
    fn probe_settings_need_rtt() {
        for key in PROBE_KEYS {
            let e = split_by_transport(&[setting(key, "1", Source::Flag)])
                .unwrap_err()
                .to_string();
            assert!(e.contains("needs rtt"), "{e}");
        }
        let usb = [setting("serial", "0B1", Source::Flag)];
        assert_eq!(split_by_transport(&usb).unwrap().0.len(), 1);
        // the ELF counters and report decode with
        let elf = [setting("elf", "fw.elf", Source::Flag)];
        assert_eq!(split_by_transport(&elf).unwrap().0.len(), 1);
        let config = format!("{:?}", client_config(&elf).unwrap());
        assert!(!config.contains("Elf"), "{config}");
    }

    #[test]
    fn probe_filter_forms() {
        let parse = |s| ProbeFilter::parse(s).unwrap();
        assert_eq!(
            parse("066DFF"),
            ProbeFilter {
                vid_pid: None,
                serial: Some("066DFF".into())
            }
        );
        assert_eq!(
            parse("0483:374b"),
            ProbeFilter {
                vid_pid: Some((0x0483, 0x374b)),
                serial: None
            }
        );
        assert_eq!(
            parse("0483:374b:066DFF"),
            ProbeFilter {
                vid_pid: Some((0x0483, 0x374b)),
                serial: Some("066DFF".into())
            }
        );
        assert!(ProbeFilter::parse("0483:xyz").is_err());

        let stlink = wire_weaver_client::ProbeInfo {
            vid: 0x0483,
            pid: 0x374b,
            serial: Some("066DFF485671".into()),
            name: "STLink V2-1".into(),
        };
        assert!(parse("066dff").matches(&stlink));
        assert!(parse("0483:374b").matches(&stlink));
        assert!(!parse("0483:374e").matches(&stlink));
        assert!(!parse("0483:374b:1234").matches(&stlink));
    }

    #[test]
    fn elf_from_ww_toml_is_relative_to_it() {
        let from_file = setting("elf", "target/fw.elf", file());
        assert_eq!(
            elf_path(&from_file),
            PathBuf::from("/work/board/target/fw.elf")
        );
        let from_flag = setting("elf", "target/fw.elf", Source::Flag);
        assert_eq!(elf_path(&from_flag), PathBuf::from("target/fw.elf"));
        let absolute = setting("elf", "/fw.elf", file());
        assert_eq!(elf_path(&absolute), PathBuf::from("/fw.elf"));
    }

    /// Device filters first, then RTT, then the probe's filters: the order `ClientConfig::rtt` reads them in.
    #[test]
    fn rtt_config_order() {
        let settings = [
            setting("api", "g0b1re_api", file()),
            setting("rtt", "STM32G0B1RETx", Source::Flag),
            setting("probe", "0483:374b:066D", Source::Flag),
            setting("elf", "fw.elf", Source::Flag),
            setting("probe_speed_khz", "4000", Source::Env),
            setting("timeout_ms", "500", Source::Flag),
        ];
        let config = format!("{:?}", client_config(&settings).unwrap());
        let at = |needle: &str| {
            config
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} in {config}"))
        };
        assert!(at("ImplementsApi") < at("Rtt {"));
        assert!(at("speed_hz: Some(4000000)") > at("Rtt {"));
        assert!(at("UsbVidPid") > at("Rtt {"));
        assert!(at("SerialContains") > at("UsbVidPid"));
        assert!(at("Elf(\"fw.elf\")") > at("SerialContains"));
        assert!(!config.contains("Usb,") && !config.contains("Usb]"));

        let zero = [
            setting("rtt", "STM32G0B1RETx", Source::Flag),
            setting("probe_speed_khz", "0", Source::Flag),
        ];
        assert!(client_config(&zero).is_err());
    }

    #[test]
    fn usb_config_without_rtt() {
        let config = format!(
            "{:?}",
            client_config(&[setting("serial", "0B1", Source::Flag)]).unwrap()
        );
        assert!(config.contains("Usb"), "{config}");
        assert!(!config.contains("Rtt"), "{config}");
    }

    /// Over RTT `api` is checked on the device once connected, over USB it picked the device already.
    #[test]
    fn api_checked_after_connecting_over_rtt() {
        use wire_weaver::ww_version::{FullVersionOwned, VersionOwned};
        let selection = |rtt: bool| {
            let mut settings = vec![setting("api", "g0b1re_api@^0.2", file())];
            if rtt {
                settings.push(setting("rtt", "STM32G0B1RETx", Source::Flag));
            }
            Selection {
                client_config: client_config(&settings).unwrap(),
                settings,
                unused: vec![],
                file: None,
                explicit_file: None,
            }
        };
        let served =
            |name: &str, minor| FullVersionOwned::new(name.into(), VersionOwned::new(0, minor, 1));
        let rtt = selection(true);
        assert!(rtt.check_connected(&served("g0b1re_api", 2)).is_ok());
        let e = rtt
            .check_connected(&served("g0b1re_api", 3))
            .unwrap_err()
            .to_string();
        assert!(e.contains("serves g0b1re_api 0.3.1"), "{e}");
        assert!(rtt.check_connected(&served("other_api", 2)).is_err());
        assert!(
            selection(false)
                .check_connected(&served("other_api", 2))
                .is_ok()
        );
    }
}
