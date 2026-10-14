use crate::cmd::api::ApiCommand;
use crate::cmd::bootsel::BootselCommand;
use crate::cmd::config::ConfigCommand;
use crate::cmd::dfu::DfuCommand;
use crate::cmd::introspect::IntrospectArgs;
use crate::cmd::list::ListArgs;
use crate::device::DeviceArgs;
use clap::{Parser, Subcommand};

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

    /// Run a USB loopback test
    USBLoopback {
        /// How long to run each test (loopback, tx speed, rx speed)
        #[arg(long, default_value = "10")]
        duration_sec: u32,

        /// Size of each test packet (number or max, capped to max USB packet size)
        #[arg(long, default_value = "max")]
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
        matches!(&self.command, Commands::Dfu(_))
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
            #[cfg(target_os = "linux")]
            Commands::Udev => false,
        }
    }
}
