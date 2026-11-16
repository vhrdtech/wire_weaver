//! The command registry: every `ww` command registers itself from its own module with `inventory::submit!`, so the
//! command line, its help, the shell completion and the run loop come from one list and no central enum names them
//! all. A crate that links this one adds its commands the same way (`ww_std_cli`: the device commands of the
//! standard APIs), and the binary that links both has every command.
//!
//! A command is a [`CommandDef`]: its `clap::Command` (built from the module's own `Args`/`Subcommand` types), which
//! global flags it shows, what it needs before it runs ([`Connection`]: nothing, the device selection, or a connected
//! device), its log level, whether it taps the connection itself and what it forwards to a plugin, and the
//! function that runs it on a [`Run`].

use crate::cmd::tap::{Attached, Layer, LiveTap};
use crate::device::Selection;
use crate::util::StopSignal;
use anyhow::{Context as _, Result, anyhow};
use clap::{ArgMatches, Command, FromArgMatches};
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

/// Which of the global flags (device selection, link tap) a subcommand shows in its help. All of them are accepted
/// everywhere, a hidden one is only left out of the help and completion.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GlobalFlags {
    /// The command opens a device connection: device selection and the link tap
    DeviceAndTap,
    /// The command finds devices, or connects without being tappable: device selection only
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
    /// `--monitor` / `--capture` may tap this command's connection (`ww explain` connects only to look an API up).
    pub tap: bool,
}

impl DeviceUse {
    /// The device's full API from its introspection data, connected before the command runs, tappable.
    pub const FULL: Self = DeviceUse {
        traits_only: false,
        on_demand: false,
        tap: true,
    };
    /// Global traits only (any device, whatever its API), connected before the command runs, tappable.
    pub const TRAITS_ONLY: Self = DeviceUse {
        traits_only: true,
        on_demand: false,
        tap: true,
    };

    /// The command connects itself with [`Run::connect`].
    pub const fn on_demand(self) -> Self {
        DeviceUse {
            on_demand: true,
            ..self
        }
    }

    /// Not tappable: `--monitor` / `--capture` are refused.
    pub const fn untapped(self) -> Self {
        DeviceUse { tap: false, ..self }
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

/// The subcommand at `path` under a command forwards its arguments to the plugin `ww-<plugin>`, as `lead` and then
/// the words typed (`ww dfu flash ARGS..` runs `ww-chunky flash ARGS..`): the completion asks the plugin for them.
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
    /// Set when a tap of any kind is on: the command ends cleanly on SIGINT / SIGTERM.
    pub stop: Option<StopSignal>,
    /// The program's name and version, for what the command prints.
    pub program: Program,
    device_use: Option<DeviceUse>,
    recorder: Option<Recorder>,
    flag_tap: Option<&'a Attached>,
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
            if let Some(attached) = self.flag_tap {
                attached.add_bundles(&device);
            }
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
    let tap_flags = crate::cmd::tap::TapFlags::from_arg_matches(&matches)
        .expect("the root command carries the tap flags");
    let connection = check(&matches)?;
    let selection = match connection {
        Connection::None => None,
        Connection::Selection { save_config } => Some(Selection::resolve(&matches, save_config)?),
        Connection::Device(_) => Some(Selection::resolve(&matches, false)?),
    };
    // the tap commands record the connection from its start: the recorder is subscribed to before connecting
    let tap = (def.own_tap)(&sub).map(LiveTap::new);
    // `--monitor` / `--capture` of a command that connects: its own tap, from the start of the connection
    let flag_tap = tap_flags
        .active()
        .then(|| Attached::start(&tap_flags))
        .transpose()?;
    let recorder = match (&tap, &flag_tap) {
        (Some(tap), _) => Some(tap.recorder.clone()),
        (None, Some(attached)) => Some(attached.recorder()),
        _ => None,
    };
    // a tapped run ends cleanly on SIGINT / SIGTERM: the capture file is finished and renamed, not left `.partial`
    let stop = (tap.is_some() || flag_tap.is_some()).then(StopSignal::install);
    // who this program is to the device and a hub: one client instance for all its connections, so a claim it takes
    // in the hub (`ww dfu`, SRV-32) lets its own calls through
    let meta = ClientMeta::new();
    let device_use = match connection {
        Connection::Device(device_use) => Some(device_use),
        _ => None,
    };
    let mut device = match (device_use, &selection) {
        (Some(device_use), Some(selection)) if !device_use.on_demand => {
            match connect_device(selection, device_use, &meta, recorder.clone(), stop.clone()).await
            {
                Ok(device) => Some(device),
                Err(e) => {
                    // the tap saw the failed connect too: let it print and close its file
                    if let Some(attached) = flag_tap
                        && let Err(tap_err) = attached.finish()
                    {
                        eprintln!("tap: {tap_err:#}");
                    }
                    return Err(e);
                }
            }
        }
        _ => None,
    };
    if let (Some(attached), Some(device)) = (&flag_tap, &device) {
        attached.add_bundles(device);
    }

    let run = Run {
        matches: sub,
        selection: selection.as_ref(),
        no_config: matches.get_flag("no_config"),
        device: &mut device,
        meta: &meta,
        tap,
        stop: stop.clone(),
        program: program(),
        device_use,
        recorder,
        flag_tap: flag_tap.as_ref(),
    };
    // a flagged command that is stopped (`observe` runs until it is) ends as if it was done: the tap is finished below
    let command_result =
        crate::util::guard(flag_tap.is_some().then_some(stop).flatten(), (def.run)(run))
            .await
            .unwrap_or(Ok(Ran::DONE));
    if let Some(attached) = flag_tap {
        // the recording ends with the disconnect
        if let Some(device) = device.take() {
            _ = device.disconnect().asynch().await;
        }
        let tap_result = attached.finish();
        match (&command_result, tap_result) {
            (Ok(_), Err(e)) => return Err(e),
            (Err(_), Err(e)) => eprintln!("tap: {e:#}"),
            _ => {}
        }
    }
    let ran = command_result?;
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

/// What the parsed command line's command needs ([`CommandDef::connection`]), after the checks that refuse a command
/// line before anything connects: the command's own, and the tap flags'.
pub(crate) fn check(matches: &ArgMatches) -> Result<Connection> {
    let (name, sub) = matches
        .subcommand()
        .expect("the root command requires a subcommand");
    let Some(def) = find(name) else {
        // a plugin connects itself
        return Ok(Connection::SELECTION);
    };
    let tap_flags = crate::cmd::tap::TapFlags::from_arg_matches(matches)
        .expect("the root command carries the tap flags");
    let connection = (def.connection)(sub)?;
    check_tap(&tap_flags, def, sub, connection)?;
    Ok(connection)
}

/// `--monitor` / `--capture` tap the command's own connection: refused where there is none, or where the command is
/// a tap itself.
fn check_tap(
    flags: &crate::cmd::tap::TapFlags,
    def: &CommandDef,
    sub: &ArgMatches,
    connection: Connection,
) -> Result<()> {
    if !flags.active() {
        return flags.check();
    }
    if (def.own_tap)(sub).is_some() {
        anyhow::bail!(
            "--monitor and --capture tap the connection of another command: `ww monitor` and `ww capture` are the tap itself, use their own options (--layer, --bytes, --bits, -o)"
        );
    }
    if !matches!(connection, Connection::Device(u) if u.tap) {
        let tappable: Vec<&str> = commands()
            .into_iter()
            .filter(|c| c.globals_of("", GlobalFlags::Neither) == GlobalFlags::DeviceAndTap)
            .map(|c| c.name)
            .collect();
        anyhow::bail!(
            "--monitor and --capture need a command that opens a device connection ({}); this one doesn't",
            tappable.join(", ")
        );
    }
    flags.check()
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
