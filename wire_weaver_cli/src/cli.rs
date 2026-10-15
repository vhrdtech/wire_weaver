use crate::cmd::api::ApiCommand;
use crate::cmd::bootsel::BootselCommand;
use crate::cmd::config::ConfigCommand;
use crate::cmd::counters::CountersArgs;
use crate::cmd::dfu::DfuCommand;
use crate::cmd::introspect::IntrospectArgs;
use crate::cmd::list::ListArgs;
use crate::complete;
use crate::device::DeviceArgs;
use clap::{Parser, Subcommand};
use clap_complete::ArgValueCandidates;

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
    },

    /// API-related tools
    #[command(subcommand)]
    Api(ApiCommand),

    /// Print the resource tree of the selected device's API, using its introspection data
    Introspect(IntrospectArgs),

    /// Firmware update steps (ww_dfu), on any device whatever its API version
    #[command(subcommand)]
    Dfu(DfuCommand),

    /// The device's cnt counters (ww_counters), decoded with the firmware's ELF found by its build id
    ///
    /// Works with any device implementing `ww_counters::Counters` anywhere in its API. Read once (default), watch
    /// changes, the TUI, reset; the same views as `cnt` shows over a debug probe.
    Counters(CountersArgs),

    /// Show or save device selection in a project ww.toml
    #[command(subcommand)]
    Config(ConfigCommand),

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

impl Cli {
    /// Commands that only call global traits connect to any device, whatever its API and version
    pub fn traits_only(&self) -> bool {
        // the loopback test does not call the API at all, the link layer answers
        matches!(
            &self.command,
            Commands::Dfu(_) | Commands::Counters(_) | Commands::USBLoopback { .. }
        )
    }

    pub fn need_device(&self) -> bool {
        match &self.command {
            Commands::List(_) => false,
            Commands::USBLoopback { .. } => true,
            Commands::Api(_) => false,
            Commands::Introspect(_) => true,
            Commands::Config(_) => false,
            Commands::Bootsel(_) => false,
            Commands::Dfu(_) => true,
            Commands::Counters(_) => true,
            #[cfg(target_os = "linux")]
            Commands::Udev => false,
        }
    }
}
