//! Blinks a device served by a host server over a byte stream: TCP, or a command's stdin and stdout.
//!
//! `cargo run -p blinky --features byte_stream --example blinky_byte_stream -- tcp 10.0.0.7:7700`, or
//! `-- command ssh nas blinky-service --stdio` to run the server through ssh.

use anyhow::Result;
use blinky::Blinky;
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    how: How,
    /// The service of the server to ask for; the server's default API when left out
    #[arg(long, global = true)]
    service: Option<String>,
}

#[derive(Subcommand)]
enum How {
    /// A TCP server at HOST:PORT
    Tcp { addr: String },
    /// The stdin and stdout of PROGRAM, started for the connection
    Command { program: String, args: Vec<String> },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let device = match args.how {
        How::Tcp { addr } => {
            // --8<-- [start:tcp]
            Blinky::config(|c| c.tcp(addr.clone()).no_ww_usb())
                .connect()
                .await?
            // --8<-- [end:tcp]
        }
        How::Command {
            program,
            args: rest,
        } => {
            // --8<-- [start:command]
            // ssh starts the server on the other machine; its stdin and stdout are the connection
            Blinky::config(|c| c.command(program.clone(), rest.clone()).no_ww_usb())
                .connect()
                .await?
            // --8<-- [end:command]
        }
    };
    device.led_on().call().await?;
    device.led_off().call().await?;
    device.disconnect().asynch().await?;
    Ok(())
}
