use crate::cmd::api::ApiCommand;
use crate::cmd::config::ConfigCommand;
use crate::cmd::counters::CountersArgs;
use crate::cmd::dfu::DfuCommand;
use crate::cmd::elf::ElfCommand;
use crate::cmd::explain::ExplainArgs;
use crate::cmd::info::{InfoArgs, ReportArgs};
use crate::cmd::introspect::IntrospectArgs;
use crate::cmd::list::ListArgs;
use crate::cmd::observe::ObserveArgs;
use crate::cmd::pins::PinsCommand;
use crate::cmd::settings::SettingsCommand;
use crate::cmd::stream::StreamArgs;
use crate::cmd::sync::SyncArgs;
use crate::cmd::tap::{CaptureArgs, MonitorArgs, StatsArgs, TapFlags};
use crate::complete;
use crate::device::DeviceArgs;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::ArgValueCandidates;
use wire_weaver_client::internal::LoopbackParams;

#[derive(Parser)]
#[command(name = "ww", version = env!("WW_VERSION"), about, long_about = None)]
#[command(propagate_version = true)]
#[command(color = clap::ColorChoice::Auto)]
#[command(styles = clap::builder::styling::Styles::styled()
    .header(clap::builder::styling::AnsiColor::Blue.on_default())
    .usage(clap::builder::styling::AnsiColor::Cyan.on_default())
    .literal(clap::builder::styling::AnsiColor::Yellow.on_default())
    .placeholder(clap::builder::styling::AnsiColor::Blue.on_default()))]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) device: DeviceArgs,

    #[command(flatten)]
    pub(crate) tap: TapFlags,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// List connected devices implementing a WireWeaver API, without opening them
    ///
    /// Only devices matching the device selection (flags, env variables and ww.toml) are shown.
    List(ListArgs),

    /// Test the link to a device: round trips, then host → device and device → host speed
    ///
    /// Works with any WireWeaver device, whatever its API: the device's link layer answers the loopback messages. Reports round trips per second with lost and corrupted
    /// echoes, then one-way messages and bytes per second in each direction.
    USBLoopback {
        /// How long to run each stage (loopback, tx_speed, rx_speed), in seconds
        #[arg(long, default_value = "10")]
        duration_sec: u32,

        /// Size of each test message in bytes (8 bytes of header and data), or max: the largest message both
        /// the device and the host accept
        #[arg(long, default_value = "max", add = ArgValueCandidates::new(complete::packet_sizes))]
        packet_size: String,

        #[command(flatten)]
        tuning: LoopbackTuning,
    },

    /// API-related tools
    #[command(subcommand)]
    Api(ApiCommand),

    /// Print the resource tree of the selected device's API, using its introspection data
    Introspect(IntrospectArgs),

    /// Which bits of a buffer hold which value: every field with its bits, raw and decoded (TOOL-9)
    ///
    /// The API comes from the device's introspection, or from --crate / --bundle without a device. Examples:
    ///
    ///   ww explain --crate api Config 0a1b2c
    ///   ww explain motor.set_speed 4a00
    ///   ww explain --bundle api.ron motor.status --return 01 80 3f
    ///   ww explain link.wwtape --layer api      a capture (ww capture), replies explained by their request
    #[command(verbatim_doc_comment)]
    Explain(ExplainArgs),

    /// Observe a trait level's `+observe` properties, from introspection alone (TOOL-11)
    ///
    /// Prints each update's present values, the update rate and the link bytes/s it costs, until Ctrl+C.
    Observe(ObserveArgs),

    /// Stream a plain, counted or reliable stream, from introspection alone (API-41)
    ///
    /// Prints one item per line (`--batches`: whole batches of a batched stream instead), until Ctrl+C. A stream
    /// function with arguments isn't supported yet.
    Stream(StreamArgs),

    /// What crosses the link, as it happens: frames, link messages or API calls, decoded (TOOL-14)
    ///
    /// Shows this command's own connection to the device, from the link setup on, until Ctrl+C. Examples:
    ///
    ///   ww monitor                    every link message and link event
    ///   ww monitor --layer frame      every USB packet with its pieces, and the messages built from them
    ///   ww monitor --layer api        requests with their arguments, replies paired with their request
    ///   ww monitor --layer api --bits each message with its bit map, as `ww explain` shows one
    #[command(verbatim_doc_comment)]
    Monitor(MonitorArgs),

    /// Framer and link statistics: frame fill, overhead, split messages, losses, reply times per method (TOOL-14)
    ///
    /// Printed every --every seconds for the time since the last print, until Ctrl+C; or of a capture. Examples:
    ///
    ///   ww stats --every 5
    ///   ww stats --file link.wwtape
    #[command(verbatim_doc_comment)]
    Stats(StatsArgs),

    /// Save what crosses the link into a file, to read later with `ww explain FILE` (TOOL-14)
    ///
    /// The file carries the device's API, so it is explained without the device. Until Ctrl+C. Examples:
    ///
    ///   ww capture -o link.wwtape
    ///   ww capture -o link.wwtape --duration 10 --layer link
    ///   ww explain link.wwtape --layer api
    #[command(verbatim_doc_comment)]
    Capture(CaptureArgs),

    /// Keep several devices' clocks on the host's UTC and show how well they follow (TOOL-16)
    ///
    /// Works with any device serving ww_timebase::Clock with host sync, anywhere in its API: each device's clock is
    /// read a few times a second, fitted against the host's (offset and rate) and steered, ten times a second while
    /// it is acquiring and once a second once locked. A device locked to a sync line only takes note. Devices with a
    /// shared clock run their animations in phase (ww_indication). Shows per device the state, the residual against
    /// the host's UTC, error, rate and slew left, a line when each and then all of them have converged, and the
    /// phase spread between them. Until q or Ctrl+C; with a ww hub running the hub steers and keeps steering after
    /// that, without one ww sync steers and the devices go on in holdover. Examples:
    ///
    ///   ww sync                          every connected device the selection matches
    ///   ww sync A1B2 bench-left          two devices by serial or label
    ///   ww sync --mode normal            and set their indication mode
    ///   ww sync --within-us 500          converged only within 0.5 ms
    #[command(verbatim_doc_comment)]
    Sync(SyncArgs),

    /// Firmware updates: ww_dfu steps on any device whatever its API version, and the RP and STM32 boot ROMs
    #[command(subcommand)]
    Dfu(DfuCommand),

    /// The ww hub: one process per user that holds devices and serves every program (SRV-13)
    #[command(subcommand)]
    Hub(crate::cmd::hub::HubCommand),

    /// The device's cnt counters (ww_counters), decoded with the firmware's ELF found by its build id
    ///
    /// Works with any device implementing `ww_counters::Counters` anywhere in its API. Read once (default), watch
    /// changes, the TUI, reset; the same views as `cnt` shows over a debug probe.
    Counters(CountersArgs),

    /// What the device is: firmware, board, ids, label and last reset, whatever its API and version
    ///
    /// Works with any device implementing the `ww_firmware_info`, `ww_board_info`, `ww_uid` and `ww_user_info`
    /// traits anywhere in its API; what it doesn't implement is said so.
    Info(InfoArgs),

    /// A bug report of the device in one file: info, counters, the stored log and last reset, decoded with its ELF
    ///
    /// The ELF is found by the build id the device reports (see `ww elf add`); parts that need an ELF that can't be
    /// found say so, the rest is still there.
    Report(ReportArgs),

    /// Pin tests on any device serving ww_pin_diag::PinDiag: shorts to GND, the supply and each other, pulls (TOOL-15)
    ///
    /// Works wherever the device's API has PinDiag, found through its introspection data. `ww pins list` shows the
    /// points, `ww pins test` tests them, with a plan file for expectations and rules.
    #[command(subcommand)]
    Pins(PinsCommand),

    /// The ELF directory: firmware ELFs stored by build id, for counters, log and crash decoding
    #[command(subcommand)]
    Elf(ElfCommand),

    /// Show or save device selection in a project ww.toml
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Tools around ww_settings::Store, device flash dumps included
    #[command(subcommand)]
    Settings(SettingsCommand),

    /// Print udev rule to the stdout, run 'ww udev --help' for more information
    ///
    /// Create udev rule:
    /// ww udev | sudo tee /etc/udev/rules.d/70-ww_device.rules
    ///
    /// Reload rules and trigger:
    /// sudo udevadm control --reload-rules
    /// sudo udevadm trigger
    #[cfg(target_os = "linux")]
    #[command(verbatim_doc_comment)]
    Udev,
}

/// Timeouts and flow control of `ww usb-loopback`, defaults from [LoopbackParams::default].
#[derive(Args)]
#[command(next_help_heading = "Test tuning")]
pub(crate) struct LoopbackTuning {
    /// An echo (round trip, marker or device → host reply) missing for this long is lost
    #[arg(long, value_name = "MS", default_value_t = LoopbackParams::default().echo_timeout.as_millis() as u64)]
    pub echo_timeout_ms: u64,

    /// Give up after this many echo timeouts in a row
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().timeouts_in_a_row)]
    pub timeouts_in_a_row: u32,

    /// Host → device: messages between two echoed markers
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().marker_every)]
    pub marker_every: u32,

    /// Host → device: markers in flight at most, the host waits for their echoes beyond that
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().markers_in_flight)]
    pub markers_in_flight: u32,

    /// Device → host: messages the device sends for one request (at least 2)
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().rx_batch)]
    pub rx_batch: u32,

    /// Device → host: requests in flight at most
    #[arg(long, value_name = "N", default_value_t = LoopbackParams::default().batches_in_flight)]
    pub batches_in_flight: u32,
}

impl LoopbackTuning {
    pub fn params(&self) -> LoopbackParams {
        LoopbackParams {
            echo_timeout: std::time::Duration::from_millis(self.echo_timeout_ms),
            timeouts_in_a_row: self.timeouts_in_a_row,
            marker_every: self.marker_every,
            markers_in_flight: self.markers_in_flight,
            rx_batch: self.rx_batch,
            batches_in_flight: self.batches_in_flight,
        }
    }
}

/// Which of the global flags (device selection, link tap) a subcommand shows in its help. All of them are
/// accepted everywhere, a hidden one is only left out of the help and completion.
#[derive(Clone, Copy, PartialEq, Debug)]
enum GlobalFlags {
    /// The command opens a device connection: device selection and the link tap
    DeviceAndTap,
    /// The command finds devices, or connects without being tappable: device selection only
    Device,
    /// The command doesn't look at devices
    Neither,
}

/// Adds the fitted copies of the global flags (`(flag, is a tap flag)`) to a subcommand at `path` (`dfu rp`) and its
/// own subcommands: hidden where it doesn't use them (by [GlobalFlags::of], else as its parent), out of the long help
/// where it does, a summary standing in for them there. A subcommand's own flag of the same name
/// (`ww dfu flash --product`) wins, as in clap's own propagation.
fn fit_globals(
    cmd: clap::Command,
    path: &str,
    parent: GlobalFlags,
    globals: &[(clap::Arg, bool)],
    header: clap::builder::styling::Style,
) -> clap::Command {
    let flags = GlobalFlags::of(path).unwrap_or(parent);
    let added: Vec<_> = globals
        .iter()
        .filter(|(a, _)| !cmd.get_arguments().any(|own| own.get_id() == a.get_id()))
        .map(|(a, is_tap)| {
            let shown = match flags {
                GlobalFlags::DeviceAndTap => true,
                GlobalFlags::Device => !is_tap,
                GlobalFlags::Neither => false,
            };
            (a.clone(), shown)
        })
        .collect();
    let summary = global_summary(&added, header);
    let cmd = added.into_iter().fold(cmd, |cmd, (a, shown)| {
        cmd.arg(match shown {
            true => a.hide_long_help(true),
            false => a.hide(true),
        })
    });
    let cmd = match summary.is_empty() {
        true => cmd,
        false => cmd.after_long_help(summary),
    };
    cmd.mut_subcommands(|sc| {
        let path = format!("{path} {}", sc.get_name());
        fit_globals(sc, &path, flags, globals, header)
    })
}

/// One line per help heading of the shown global flags, `-s/--serial, --api, ...`, and where their help is.
fn global_summary(globals: &[(clap::Arg, bool)], header: clap::builder::styling::Style) -> String {
    let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
    for (a, _) in globals.iter().filter(|(_, shown)| *shown) {
        let heading = a.get_help_heading().unwrap_or("Options");
        let flag = match (a.get_short(), a.get_long()) {
            (Some(s), Some(l)) => format!("-{s}/--{l}"),
            (None, Some(l)) => format!("--{l}"),
            (Some(s), None) => format!("-{s}"),
            (None, None) => continue,
        };
        match groups.iter_mut().find(|(h, _)| *h == heading) {
            Some((_, flags)) => flags.push(flag),
            None => groups.push((heading, vec![flag])),
        }
    }
    if groups.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (heading, flags) in groups {
        out += &format!("{header}{heading}:{header:#} {}\n", flags.join(", "));
    }
    out + "Global flags: `-h` shows them with their help, `ww --help` in full."
}

impl GlobalFlags {
    /// By the subcommand's path (`dfu rp`); `None` leaves it to the parent's.
    fn of(path: &str) -> Option<Self> {
        use GlobalFlags::*;
        Some(match path {
            "usb-loopback" | "introspect" | "observe" | "stream" | "dfu" | "counters" | "info"
            | "report" | "pins" => DeviceAndTap,
            "list" | "sync" | "config" | "dfu rp" | "dfu stm32" | "explain" | "monitor"
            | "stats" | "capture" => Device,
            "api" | "elf" | "settings" | "hub" | "udev" => Neither,
            _ => return None,
        })
    }
}

impl Cli {
    /// The command line, with the global flags fitted to each subcommand's help. A subcommand shows only the groups
    /// it uses; in its `--help` they are one summary line per group, with their help in `-h` and in `ww --help`.
    /// Clap's long layout gives every flag two lines or more plus a blank one, so the 15 global flags took ~60 lines
    /// of every subcommand's `--help`, even `ww udev --help`.
    ///
    /// Clap copies a global flag into a subcommand only when the subcommand doesn't have it yet, so the fitted
    /// copies added here are what the subcommands (and theirs) get; they stay global and parse as before.
    pub(crate) fn command_fitted() -> clap::Command {
        let root = Cli::command();
        let tap = TapFlags::augment_args(clap::Command::new("tap"));
        let globals: Vec<(clap::Arg, bool)> = root
            .get_arguments()
            .filter(|a| a.is_global_set())
            .map(|a| {
                (
                    a.clone(),
                    tap.get_arguments().any(|t| t.get_id() == a.get_id()),
                )
            })
            .collect();
        let header = *root.get_styles().get_header();
        // a subcommand added without a line in `GlobalFlags::of` shows everything (a test points it out)
        root.mut_subcommands(|sc| {
            let path = sc.get_name().to_string();
            fit_globals(sc, &path, GlobalFlags::DeviceAndTap, &globals, header)
        })
    }

    /// Commands that only call global traits connect to any device, whatever its API and version. `report` reads
    /// the device's introspection data as well: the stored log is a stream, opened on the trait's absolute path.
    pub fn traits_only(&self) -> bool {
        // the loopback test does not call the API at all, the link layer answers
        matches!(
            &self.command,
            Commands::Dfu(DfuCommand::Device(_))
                | Commands::Counters(_)
                | Commands::Info(_)
                | Commands::USBLoopback { .. }
        )
    }

    /// `--monitor` / `--capture` tap the command's own connection: refused where there is none, or where the
    /// command is a tap itself.
    pub fn check_tap(&self) -> anyhow::Result<()> {
        if !self.tap.active() {
            return self.tap.check();
        }
        let own_tap = match &self.command {
            Commands::Monitor(_) | Commands::Capture(_) => true,
            Commands::Stats(args) => args.file.is_none(),
            _ => false,
        };
        if own_tap {
            anyhow::bail!(
                "--monitor and --capture tap the connection of another command: `ww monitor` and `ww capture` are the tap itself, use their own options (--layer, --bytes, --bits, -o)"
            );
        }
        // `ww explain` may connect, but only to look an API up
        if matches!(self.command, Commands::Explain(_)) || !self.need_device() {
            anyhow::bail!(
                "--monitor and --capture need a command that opens a device connection (info, observe, stream, introspect, dfu, counters, report, pins, usb-loopback); this one doesn't"
            );
        }
        self.tap.check()
    }

    pub fn need_device(&self) -> bool {
        match &self.command {
            Commands::List(_) => false,
            Commands::Sync(_) => false,
            Commands::USBLoopback { .. } => true,
            Commands::Api(_) => false,
            Commands::Introspect(_) => true,
            Commands::Explain(args) => args.need_device(),
            Commands::Observe(_) => true,
            Commands::Stream(_) => true,
            Commands::Monitor(_) | Commands::Capture(_) => true,
            Commands::Stats(args) => args.file.is_none(),
            Commands::Config(_) => false,
            Commands::Dfu(dfu) => dfu.needs_device(),
            Commands::Counters(_) | Commands::Info(_) | Commands::Report(_) => true,
            Commands::Pins(_) => true,
            Commands::Elf(_) => false,
            Commands::Settings(_) => false,
            Commands::Hub(_) => false,
            #[cfg(target_os = "linux")]
            Commands::Udev => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::FromArgMatches;

    fn help(args: &[&str]) -> String {
        let err = Cli::command_fitted()
            .try_get_matches_from(std::iter::once(&"ww").chain(args))
            .expect_err("help is an early exit");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        err.render().to_string()
    }

    #[test]
    fn fitted_command_is_valid() {
        Cli::command_fitted().debug_assert();
    }

    #[test]
    fn every_subcommand_says_which_global_flags_it_shows() {
        for sc in Cli::command().get_subcommands() {
            assert!(
                GlobalFlags::of(sc.get_name()).is_some(),
                "`ww {}` is missing in GlobalFlags::of",
                sc.get_name()
            );
        }
    }

    #[test]
    fn global_flags_shown_only_where_used() {
        assert!(!help(&["elf", "--help"]).contains("--serial"));
        assert!(!help(&["elf", "-h"]).contains("--serial"));
        let list = help(&["list", "-h"]);
        assert!(list.contains("--serial") && !list.contains("--monitor"));
        let info = help(&["info", "-h"]);
        assert!(info.contains("[env: WW_SERIAL=]") && info.contains("--monitor"));
        // the boot ROMs are found by device selection, without a connection to tap
        let rp = help(&["dfu", "rp", "list", "-h"]);
        assert!(rp.contains("--serial") && !rp.contains("--monitor"), "{rp}");
        assert!(help(&["dfu", "stm32", "flash", "--help"]).contains("-s/--serial"));
        assert!(!help(&["dfu", "stm32", "flash", "--help"]).contains("--monitor"));
        assert!(help(&["dfu", "status", "-h"]).contains("--monitor"));
        // a subcommand's own flag of the same name stays its own
        assert!(help(&["dfu", "flash", "-h"]).contains("--product <PRODUCT>        Product id"));
    }

    #[test]
    fn long_help_sums_global_flags_up() {
        let info = help(&["info", "--help"]);
        assert!(!info.contains("[env: WW_SERIAL=]"), "{info}");
        assert!(info.contains("-s/--serial, -l/--label"), "{info}");
        assert!(info.contains("--monitor, --monitor-bytes"), "{info}");
        assert!(!help(&["list", "--help"]).contains("--monitor"));
        assert!(!help(&["dfu", "flash", "--help"]).contains("-p/--product"));
        // in full where they are defined
        assert!(help(&["--help"]).contains("[env: WW_SERIAL=]"));
    }

    #[test]
    fn hidden_global_flags_still_parse() {
        for args in [
            &["ww", "-s", "x", "elf", "id", "f.elf"][..],
            &["ww", "elf", "id", "f.elf", "-s", "x"],
        ] {
            let matches = Cli::command_fitted().try_get_matches_from(args).unwrap();
            assert!(Cli::from_arg_matches(&matches).is_ok());
        }
    }
}
