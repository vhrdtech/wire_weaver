use crate::cmd::api::ApiCommand;
use crate::cmd::bootsel::BootselCommand;
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
use crate::cmd::tap::{CaptureArgs, MonitorArgs, StatsArgs, TapFlags};
use crate::complete;
use crate::device::DeviceArgs;
use clap::{Args, Parser, Subcommand};
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

    /// Firmware update steps (ww_dfu), on any device whatever its API version
    #[command(subcommand)]
    Dfu(DfuCommand),

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

    /// RP2040 / RP2350 boot ROM in BOOTSEL mode: list, flash over PICOBOOT, reboot
    ///
    /// --serial selects the device, as for other commands (the RP2350 boot ROM reports the chip id).
    #[command(subcommand)]
    Bootsel(BootselCommand),

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

impl Cli {
    /// Commands that only call global traits connect to any device, whatever its API and version. `report` reads
    /// the device's introspection data as well: the stored log is a stream, opened on the trait's absolute path.
    pub fn traits_only(&self) -> bool {
        // the loopback test does not call the API at all, the link layer answers
        matches!(
            &self.command,
            Commands::Dfu(_)
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
            Commands::USBLoopback { .. } => true,
            Commands::Api(_) => false,
            Commands::Introspect(_) => true,
            Commands::Explain(args) => args.need_device(),
            Commands::Observe(_) => true,
            Commands::Stream(_) => true,
            Commands::Monitor(_) | Commands::Capture(_) => true,
            Commands::Stats(args) => args.file.is_none(),
            Commands::Config(_) => false,
            Commands::Bootsel(_) => false,
            Commands::Dfu(_) => true,
            Commands::Counters(_) | Commands::Info(_) | Commands::Report(_) => true,
            Commands::Pins(_) => true,
            Commands::Elf(_) => false,
            Commands::Settings(_) => false,
            #[cfg(target_os = "linux")]
            Commands::Udev => false,
        }
    }
}
