use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(color = clap::ColorChoice::Auto)]
#[command(styles = clap::builder::styling::Styles::styled()
    .header(clap::builder::styling::AnsiColor::Blue.on_default())
    .usage(clap::builder::styling::AnsiColor::Cyan.on_default())
    .literal(clap::builder::styling::AnsiColor::Yellow.on_default())
    .placeholder(clap::builder::styling::AnsiColor::Blue.on_default()))]
pub(crate) struct Cli {
    /// Serial number of a device to use, can use partial serial number if the result is unique, can not be used together with usb_path
    #[arg(short, long, group = "device-selection")]
    pub(crate) serial: Option<String>,
    // Usb path of a device to use, not to be used with serial number selector
    // #[arg(short, long, value_parser = from_arg_usb_path, group = "dongle-selection",
    //       value_name = "bus:port-chain")]
    // usb_path: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Run USB loopback test
    USBLoopback,

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
