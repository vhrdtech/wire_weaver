//! The command registry: every `ww` command registers itself from its own module with `inventory::submit!`, so the
//! command line, its help, the shell completion and the run loop come from one list and no central enum names them
//! all. A crate that links this one adds its commands the same way (`ww_std_cli`: the device commands of the
//! standard APIs), and the binary that links both has every command.
//!
//! A command is a [`CommandDef`]: its `clap::Command` (built from the module's own `Args`/`Subcommand` types), which
//! global flags it shows, what it needs before it runs ([`Connection`]: nothing, the device selection, or a connected
//! device), its log level, whether it taps the connection itself and what it forwards to a plugin, and the
//! function that runs it on a [`Run`].

use crate::cmd::tap::{Layer, LiveTap};
use crate::device::Selection;
use crate::util::StopSignal;
use anyhow::{Context as _, Result, anyhow};
use clap::{ArgMatches, Command};
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use tracing::level_filters::LevelFilter;
use wire_weaver_client::tap::Recorder;
use wire_weaver_client::{ClientMeta, DynClient};

/// A future a command returns from its `run`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// What the binary is: its name and its `--version` text (version, git SHA and commit time, from the binary's build
/// script: `ww 0.5.0 (a1b2c3d-dirty, committed 3 Oct 2026 18:20)`).
#[derive(Clone, Copy, Debug)]
pub struct Program {
    pub name: &'static str,
    pub version: &'static str,
}

static PROGRAM: OnceLock<Program> = OnceLock::new();

/// Sets what the binary is, once, before the command line is built; [`program`] then gives it.
pub fn set_program(program: Program) {
    let _ = PROGRAM.set(program);
}

/// The program as the binary set it; `ww` with the crate version until it did (a test).
pub fn program() -> Program {
    *PROGRAM.get_or_init(|| Program {
        name: "ww",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Which of the global flags (device selection) a subcommand shows in its help. All of them are accepted
/// everywhere, a hidden one is only left out of the help and completion.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GlobalFlags {
    /// The command finds devices or opens a device connection: device selection
    Device,
    /// The command doesn't look at devices
    Neither,
}

/// How a command uses a device connection.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DeviceUse {
    /// The command only calls global traits (`ww dfu`, `ww info`): connect to any device, whatever its API and
    /// version, without its introspection data.
    pub traits_only: bool,
    /// The command connects itself, with [`Run::connect`], once it knows it needs to (`ww info` first looks for a
    /// device sitting in its boot ROM); otherwise the device is connected before the command runs.
    pub on_demand: bool,
}

impl DeviceUse {
    /// The device's full API from its introspection data, connected before the command runs.
    pub const FULL: Self = DeviceUse {
        traits_only: false,
        on_demand: false,
    };
    /// Global traits only (any device, whatever its API), connected before the command runs.
    pub const TRAITS_ONLY: Self = DeviceUse {
        traits_only: true,
        on_demand: false,
    };

    /// The command connects itself with [`Run::connect`].
    pub const fn on_demand(self) -> Self {
        DeviceUse {
            on_demand: true,
            ..self
        }
    }
}

/// What a command needs before it runs, from its arguments.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Connection {
    /// Nothing: the command doesn't look at devices (`ww api`, `ww elf`).
    None,
    /// The device selection (flags, `WW_*` variables, `ww.toml`), resolved but not connected: the command lists or
    /// finds devices itself (`ww list`, `ww dfu rp`), or hands the selection to a plugin.
    Selection {
        /// `ww config save` writes the file: a missing one is no error.
        save_config: bool,
    },
    /// A device connection as [`DeviceUse`] says.
    Device(DeviceUse),
}

impl Connection {
    /// The device selection, resolved (`ww list`).
    pub const SELECTION: Self = Connection::Selection { save_config: false };
}

/// The subcommand at `path` under a command forwards its arguments to the command `plugin`, as `lead` and then the
/// words typed (`ww dfu flash ARGS..` runs `ww chunky flash ARGS..`): in this process when the binary links a
/// command of that name ([`forward`]), else the plugin `ww-<plugin>`. The completion asks the same one.
#[derive(Clone, Copy, Debug)]
pub struct Forward {
    /// The subcommand's words after the command's name (`&["flash"]` for `ww dfu flash`).
    pub path: &'static [&'static str],
    /// The plugin's name, `ww-<plugin>` on PATH.
    pub plugin: &'static str,
    /// The words the plugin gets before the typed ones.
    pub lead: &'static [&'static str],
}

/// How a command ended.
#[derive(Clone, Copy, Debug)]
pub struct Ran {
    /// `false` when the device is gone or about to be (nothing to disconnect from).
    pub disconnect: bool,
    /// `false` when a test found a fault (`ww pins test`): exit code 1.
    pub passed: bool,
}

impl Ran {
    /// Done, the device disconnected after.
    pub const DONE: Self = Ran {
        disconnect: true,
        passed: true,
    };
    /// The device is gone or about to be (a reboot): nothing to disconnect from.
    pub const DEVICE_GONE: Self = Ran {
        disconnect: false,
        passed: true,
    };
}

/// One `ww` subcommand, registered from its module with `inventory::submit!`.
pub struct CommandDef {
    /// The subcommand's name on the command line.
    pub name: &'static str,
    /// Its place in the help, lower first.
    pub order: u16,
    /// Builds the subcommand: its about text, arguments and subcommands.
    pub command: fn() -> Command,
    /// Which global flags each subcommand path shows in its help: `""` the command itself, `"rp"` for `ww dfu rp`.
    /// A path not named shows its parent's.
    pub globals: &'static [(&'static str, GlobalFlags)],
    /// What the command needs before it runs, from its arguments; an error refuses the arguments before anything
    /// connects.
    pub connection: fn(&ArgMatches) -> Result<Connection>,
    /// The most verbose log level shown while it runs (`INFO` by default; `WARN` keeps connection progress out of a
    /// printed tree).
    pub log_level: fn(&ArgMatches) -> LevelFilter,
    /// The layer the command taps its own connection at (`ww monitor`, `ww capture`): the tap is set up before
    /// connecting and handed to the command in [`Run::tap`].
    pub own_tap: fn(&ArgMatches) -> Option<Layer>,
    /// Subcommands that forward their arguments to a plugin.
    pub forwards: &'static [Forward],
    /// Runs the command.
    pub run: for<'a> fn(Run<'a>) -> BoxFuture<'a, Result<Ran>>,
}

fn connection_none(_: &ArgMatches) -> Result<Connection> {
    Ok(Connection::None)
}

fn log_info(_: &ArgMatches) -> LevelFilter {
    LevelFilter::INFO
}

fn no_own_tap(_: &ArgMatches) -> Option<Layer> {
    None
}

impl CommandDef {
    /// A command that needs no device and shows no device flags; the other fields by their methods.
    pub const fn new(
        name: &'static str,
        order: u16,
        command: fn() -> Command,
        run: for<'a> fn(Run<'a>) -> BoxFuture<'a, Result<Ran>>,
    ) -> Self {
        CommandDef {
            name,
            order,
            command,
            globals: &[("", GlobalFlags::Neither)],
            connection: connection_none,
            log_level: log_info,
            own_tap: no_own_tap,
            forwards: &[],
            run,
        }
    }

    pub const fn globals(self, globals: &'static [(&'static str, GlobalFlags)]) -> Self {
        CommandDef { globals, ..self }
    }

    pub const fn connection(self, connection: fn(&ArgMatches) -> Result<Connection>) -> Self {
        CommandDef { connection, ..self }
    }

    pub const fn log_level(self, log_level: fn(&ArgMatches) -> LevelFilter) -> Self {
        CommandDef { log_level, ..self }
    }

    pub const fn own_tap(self, own_tap: fn(&ArgMatches) -> Option<Layer>) -> Self {
        CommandDef { own_tap, ..self }
    }

    pub const fn forwards(self, forwards: &'static [Forward]) -> Self {
        CommandDef { forwards, ..self }
    }

    /// The name of the subcommand `forward` stands at: the last word of its path, the command's own name for an
    /// empty path.
    pub(crate) fn name_at(&self, forward: &Forward) -> &'static str {
        forward.path.last().copied().unwrap_or(self.name)
    }

    /// The global flags a subcommand path shows: its own entry, else `parent`.
    pub(crate) fn globals_of(&self, path: &str, parent: GlobalFlags) -> GlobalFlags {
        self.globals
            .iter()
            .find(|(p, _)| *p == path)
            .map_or(parent, |(_, g)| *g)
    }
}

inventory::collect!(CommandDef);

/// Every registered command, by their order in the help.
pub fn commands() -> Vec<&'static CommandDef> {
    let mut all: Vec<_> = inventory::iter::<CommandDef>().collect();
    all.sort_by_key(|c| (c.order, c.name));
    all
}

/// The registered command of that name.
pub fn find(name: &str) -> Option<&'static CommandDef> {
    inventory::iter::<CommandDef>().find(|c| c.name == name)
}

impl Forward {
    /// The registered command the forward runs in this process, when the binary links one named `plugin`.
    pub fn registered(&self) -> Option<&'static CommandDef> {
        find(self.plugin)
    }

    /// The command line of the forwarded command: `plugin`, `lead`, then the words typed.
    pub(crate) fn command_line(&self, args: &[std::ffi::OsString]) -> Vec<std::ffi::OsString> {
        std::iter::once(self.plugin)
            .chain(self.lead.iter().copied())
            .map(std::ffi::OsString::from)
            .chain(args.iter().cloned())
            .collect()
    }
}

/// Runs `forward` with the words typed after its subcommand (`ww dfu flash ARGS..`): the registered command in this
/// process when the binary links it, with the device selection of `run` and a connection of its own; else the plugin
/// `ww-<plugin>` instead of this process (`missing` is the error when there is none). A parse error or `--help` of
/// the registered command ends the process, as `ww`'s own command line does.
pub async fn forward(
    run: Run<'_>,
    forward: &Forward,
    args: &[std::ffi::OsString],
    missing: impl FnOnce() -> anyhow::Error,
) -> Result<Ran> {
    let Some(def) = forward.registered() else {
        let mut plugin_args: Vec<std::ffi::OsString> =
            forward.lead.iter().map(std::ffi::OsString::from).collect();
        plugin_args.extend(args.iter().cloned());
        let env = crate::plugin::selection_env(run.selection(), run.no_config);
        match crate::plugin::run(forward.plugin, &plugin_args, &env, missing)? {}
    };
    let root = forwarded_matches(run.selection(), run.no_config, forward, args)
        .unwrap_or_else(|e| e.exit());
    let (_, matches) = root
        .subcommand()
        .expect("the forwarded command line names the command");
    let matches = matches.clone();
    let selection = Selection::resolve(&root, false)?;
    let connection = (def.connection)(&matches)?;
    let device_use = match connection {
        Connection::Device(device_use) => Some(device_use),
        Connection::None | Connection::Selection { .. } => None,
    };
    let mut device = match device_use {
        Some(device_use) if !device_use.on_demand => {
            Some(connect_device(&selection, device_use, run.meta, None, run.stop.clone()).await?)
        }
        _ => None,
    };
    let forwarded = Run {
        matches,
        selection: Some(&selection),
        no_config: root.get_flag("no_config"),
        device: &mut device,
        meta: run.meta,
        tap: None,
        stop: run.stop.clone(),
        program: run.program,
        device_use,
        recorder: None,
    };
    let ran = (def.run)(forwarded).await?;
    if ran.disconnect
        && let Some(device) = device
    {
        device.disconnect().asynch().await?;
    }
    Ok(ran)
}

/// `ww`'s command line for `forward` with the words typed after its subcommand: `ww <plugin> <lead>.. <words>..`,
/// with the device flags given before the forwarding subcommand (`outer`'s, from the command line) in front, except
/// those given again among the words, which win as they do for a plugin.
fn forwarded_matches(
    outer: &Selection,
    no_config: bool,
    forward: &Forward,
    args: &[std::ffi::OsString],
) -> Result<ArgMatches, clap::Error> {
    let program = program().name;
    let tail = forward.command_line(args);
    let after = crate::cli::root_command().try_get_matches_from(
        std::iter::once(std::ffi::OsString::from(program)).chain(tail.clone()),
    )?;
    let typed_after = |id: &str| {
        matches!(
            after.value_source(id),
            Some(clap::parser::ValueSource::CommandLine)
        )
    };
    let root = crate::cli::root_command();
    let long_of = |id: &str| {
        root.get_arguments()
            .find(|a| a.get_id() == id)
            .and_then(|a| a.get_long())
            .map(str::to_owned)
    };
    let mut line = vec![std::ffi::OsString::from(program)];
    for setting in &outer.settings {
        if matches!(setting.source, crate::device::Source::Flag)
            && !typed_after(setting.key)
            && let Some(long) = long_of(setting.key)
        {
            line.push(format!("--{long}={}", setting.value).into());
        }
    }
    if let Some(config) = &outer.explicit_file
        && !typed_after("config")
    {
        let mut flag = std::ffi::OsString::from("--config=");
        flag.push(config);
        line.push(flag);
    }
    if no_config && !typed_after("no_config") {
        line.push("--no-config".into());
    }
    line.extend(tail);
    root.try_get_matches_from(line)
}

/// What a command runs on: its arguments, the device selection and connection the registry set up for it, and
/// what the run loop needs back.
pub struct Run<'a> {
    /// The subcommand's own matches.
    pub matches: ArgMatches,
    /// The device selection, when the command's [`Connection`] asked for it.
    pub selection: Option<&'a Selection>,
    /// `--no-config`: the plugin a command forwards to is told so.
    pub no_config: bool,
    /// The connected device, when the command's [`Connection`] asked for one and it isn't on demand; a command may
    /// take it (a reboot, a tap that disconnects itself).
    pub device: &'a mut Option<DynClient>,
    /// Who this program is to the device and a hub: one client instance for all its connections.
    pub meta: &'a ClientMeta,
    /// The command's own tap, from [`CommandDef::own_tap`].
    pub tap: Option<LiveTap>,
    /// Set when the command taps its own connection: it ends cleanly on SIGINT / SIGTERM.
    pub stop: Option<StopSignal>,
    /// The program's name and version, for what the command prints.
    pub program: Program,
    device_use: Option<DeviceUse>,
    recorder: Option<Recorder>,
}

impl<'a> Run<'a> {
    /// The connected device: a command whose [`Connection`] asked for one, not on demand.
    ///
    /// # Panics
    ///
    /// When the command's `connection` didn't ask for a device before the run: that is the command's own
    /// contract, not an error of operation.
    pub fn device(&self) -> &DynClient {
        self.device
            .as_ref()
            .expect("the command's connection asked for a device before it ran")
    }

    /// The device selection: a command whose [`Connection`] asked for it or for a device.
    ///
    /// # Panics
    ///
    /// When the command's `connection` asked for neither.
    pub fn selection(&self) -> &'a Selection {
        self.selection
            .expect("the command's connection asked for the device selection")
    }

    /// Connects the on-demand device ([`DeviceUse::on_demand`]) the way the run loop connects the others: the
    /// selection's client config, this program's client instance, the tap when one is on.
    pub async fn connect(&mut self) -> Result<&mut DynClient> {
        if self.device.is_none() {
            let selection = self.selection();
            let device_use = self
                .device_use
                .context("the command's connection didn't ask for a device")?;
            let device = connect_device(
                selection,
                device_use,
                self.meta,
                self.recorder.clone(),
                self.stop.clone(),
            )
            .await?;
            *self.device = Some(device);
        }
        Ok(self.device.as_mut().expect("just connected"))
    }
}

/// Connects to the selected device as `device_use` says, recording into `recorder` when there is one, giving up
/// when `stop` is asked for.
pub(crate) async fn connect_device(
    selection: &Selection,
    device_use: DeviceUse,
    meta: &ClientMeta,
    recorder: Option<Recorder>,
    stop: Option<StopSignal>,
) -> Result<DynClient> {
    let config = if device_use.traits_only {
        selection.client_config().traits_only()
    } else {
        selection.client_config()
    };
    let config = config.client_meta(meta.clone());
    let config = match recorder {
        Some(recorder) => config.tap(recorder),
        None => config,
    };
    let connect = async {
        DynClient::from_config(config)
            .connect()
            .await
            .with_context(|| {
                format!(
                    "connecting to device ({selection}), run 'ww list{}' to see connected {}",
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
            })
    };
    let device = crate::util::guard(stop, connect)
        .await
        .unwrap_or_else(|| Err(anyhow!("stopped while connecting")))?;
    // over RTT the device behind the probe is only known now
    selection.check_connected(&device.device_api_info().user_api_version)?;
    Ok(device)
}

/// Runs the command line: the registered commands, a plugin for any other name (see [`crate::plugin`]).
pub(crate) async fn run() -> Result<()> {
    let root = crate::cli::root_command();
    let matches = root.get_matches();
    let (name, sub) = matches
        .subcommand()
        .expect("the root command requires a subcommand");
    let Some(def) = find(name) else {
        // `ww NAME ARGS..` with no such command: the plugin `ww-NAME`
        let args: Vec<std::ffi::OsString> = sub
            .get_many::<std::ffi::OsString>("")
            .map(|v| v.cloned().collect())
            .unwrap_or_default();
        let selection = Selection::resolve(&matches, false)?;
        let env = crate::plugin::selection_env(&selection, matches.get_flag("no_config"));
        match crate::plugin::run(name, &args, &env, || {
            anyhow!(
                "no such command: `ww {name}`, and no `ww-{name}` on PATH (see 'Plugins' in the CLI reference)"
            )
        })? {}
    };
    let sub = sub.clone();
    init_logging((def.log_level)(&sub));
    let connection = (def.connection)(&sub)?;
    let selection = match connection {
        Connection::None => None,
        Connection::Selection { save_config } => Some(Selection::resolve(&matches, save_config)?),
        Connection::Device(_) => Some(Selection::resolve(&matches, false)?),
    };
    // the tap commands record the connection from its start: the recorder is subscribed to before connecting
    let tap = (def.own_tap)(&sub).map(LiveTap::new);
    let recorder = tap.as_ref().map(|tap| tap.recorder.clone());
    // a tapped run ends cleanly on SIGINT / SIGTERM: the capture file is finished and renamed, not left `.partial`
    let stop = tap.as_ref().map(|_| StopSignal::install());
    // who this program is to the device and a hub: one client instance for all its connections, so a claim it takes
    // in the hub (`ww dfu`, SRV-32) lets its own calls through
    let meta = ClientMeta::new();
    let device_use = match connection {
        Connection::Device(device_use) => Some(device_use),
        _ => None,
    };
    let mut device = match (device_use, &selection) {
        (Some(device_use), Some(selection)) if !device_use.on_demand => Some(
            connect_device(selection, device_use, &meta, recorder.clone(), stop.clone()).await?,
        ),
        _ => None,
    };

    let run = Run {
        matches: sub,
        selection: selection.as_ref(),
        no_config: matches.get_flag("no_config"),
        device: &mut device,
        meta: &meta,
        tap,
        stop,
        program: program(),
        device_use,
        recorder,
    };
    let ran = (def.run)(run).await?;
    if ran.disconnect
        && let Some(device) = device
    {
        device.disconnect().asynch().await?;
    }
    if !ran.passed {
        // a test that found a fault, after disconnecting
        std::process::exit(1);
    }
    Ok(())
}

fn init_logging(max_level: LevelFilter) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    // probe-rs says what it does with the probe (`Halting core 0...`) at INFO, `--rtt` shows its warnings only
    let probe_rs_level = max_level.min(LevelFilter::WARN);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::filter::Targets::new()
                .with_default(max_level)
                .with_target("probe_rs", probe_rs_level),
        )
        .init();
}

/// The device selection as a command run with no flags would see it (environment variables and `ww.toml`): for a
/// completion that asks a device.
pub fn selection_from_env() -> Result<Selection> {
    let matches = crate::cli::root_command().try_get_matches_from([program().name, "list"])?;
    Selection::resolve(&matches, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A forward to `ww info`, a command this crate registers.
    const TO_INFO: Forward = Forward {
        path: &["go"],
        plugin: "info",
        lead: &[],
    };

    /// The device selection of `ww FLAGS.. list`, without a ww.toml.
    fn outer(flags: &[&str]) -> Selection {
        let line = ["ww", "--no-config"].iter().chain(flags).chain(&["list"]);
        let matches = crate::cli::root_command()
            .try_get_matches_from(line)
            .expect("ww parses the command line");
        Selection::resolve(&matches, false).expect("a selection")
    }

    fn forwarded(outer: &Selection, words: &[&str]) -> ArgMatches {
        let words: Vec<std::ffi::OsString> = words.iter().map(Into::into).collect();
        forwarded_matches(outer, true, &TO_INFO, &words).expect("the forwarded command line parses")
    }

    /// Device flags before the forwarding command reach the forwarded one, next to those typed after it.
    #[test]
    fn flags_before_and_after_both_count() {
        let root = forwarded(&outer(&["-s", "0B135A01"]), &["--label", "bench"]);
        assert_eq!(root.subcommand_name(), Some("info"));
        let selection = Selection::resolve(&root, false).expect("a selection");
        assert_eq!(selection.get("serial"), Some("0B135A01"));
        assert_eq!(selection.get("label"), Some("bench"));
    }

    /// A flag typed again after the forwarding command wins, as it does for a plugin, and is no duplicate.
    #[test]
    fn a_flag_typed_after_wins() {
        let root = forwarded(&outer(&["-s", "AAAA"]), &["-s", "BBBB"]);
        let selection = Selection::resolve(&root, false).expect("a selection");
        assert_eq!(selection.get("serial"), Some("BBBB"));
    }

    /// The forwarded command's own errors are its own: an unknown flag is refused.
    #[test]
    fn an_unknown_flag_is_refused() {
        let words = vec![std::ffi::OsString::from("--no-such-flag")];
        let err = forwarded_matches(&outer(&[]), true, &TO_INFO, &words).expect_err("unknown flag");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn a_registered_target_is_found() {
        assert!(TO_INFO.registered().is_some());
        let to_plugin = Forward {
            plugin: "no-such-command",
            ..TO_INFO
        };
        assert!(to_plugin.registered().is_none());
    }
}
