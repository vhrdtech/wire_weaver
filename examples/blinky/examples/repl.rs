use anyhow::Result;
use blinky::{Blinky, DeviceFilter, OnError};
use clap::Parser;
use clap_repl::reedline::{DefaultPrompt, DefaultPromptSegment, FileBackedHistory};
use clap_repl::{ClapEditor, ReadCommandOutput};
use console::style;
use std::time::Duration;
use tracing::{error, info};

#[derive(Parser)]
enum Command {
    Connect,
    Disconnect,
    Exit,

    LedOn,
    LedOff,
    Blink { count: u32, delay_ms: u32 },
    // Add your commands here.
    // Arguments can be added like so: MyCommand { x: f32 },
}

async fn handle_command(device: &mut Blinky, cmd: Command) -> Result<()> {
    match cmd {
        Command::LedOn => {
            device.led_on().call().await?;
        }
        Command::LedOff => {
            device.led_off().call().await?;
        }
        Command::Blink { count, delay_ms } => {
            for _ in 0..count {
                println!("On");
                device.led_on().call().await?;
                tokio::time::sleep(Duration::from_millis(delay_ms as u64)).await;
                println!("Off");
                device.led_off().call().await?;
                tokio::time::sleep(Duration::from_millis(delay_ms as u64)).await;
            }
        }
        // Handle additional commands here

        // Already handled
        Command::Connect | Command::Disconnect | Command::Exit => {}
    }
    Ok(())
}

async fn connect_to_device() -> Result<Blinky> {
    let filter = DeviceFilter::usb_vid_pid(0xc0de, 0xcafe);
    let device = Blinky::connect(filter, OnError::ExitImmediately).await?;
    Ok(device)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let mut device = None;
    let mut rl = setup_repl();

    loop {
        let should_exit = handle_user_command(&mut device, &mut rl).await?;
        if should_exit {
            break;
        }
    }

    if let Some(d) = device.as_mut() {
        d.disconnect_and_exit().await?;
    }
    Ok(())
}

async fn handle_user_command(
    device: &mut Option<Blinky>,
    rl: &mut ClapEditor<Command>,
) -> Result<bool> {
    match rl.read_command() {
        ReadCommandOutput::Command(c) => match c {
            Command::Connect => match connect_to_device().await {
                Ok(d) => {
                    info!("Connected!");
                    *device = Some(d);
                }
                Err(e) => {
                    error!("{}", e);
                }
            },
            Command::Disconnect => {
                if let Some(mut d) = device.take() {
                    let r = d.disconnect_and_exit().await;
                    if r.is_ok() {
                        info!("Disconnect: {r:?}");
                    } else {
                        error!("Disconnect: {r:?}");
                    }
                } else {
                    info!("Already disconnected");
                }
            }
            Command::Exit => return Ok(true),
            c => {
                let Some(d) = device.as_mut() else {
                    println!("{}", style("No connection, connect first").yellow());
                    return Ok(false);
                };
                let r = handle_command(d, c).await;
                match r {
                    Ok(_) => info!("ok"),
                    Err(e) => error!("{e}"),
                }
            }
        },
        ReadCommandOutput::EmptyLine => (),
        ReadCommandOutput::ClapError(e) => {
            e.print()?;
        }
        ReadCommandOutput::ShlexError => {
            println!(
                "{} input was not valid and could not be processed",
                style("Error:").red().bold()
            );
        }
        ReadCommandOutput::ReedlineError(e) => {
            panic!("{e}");
        }
        ReadCommandOutput::CtrlC => return Ok(true),
        ReadCommandOutput::CtrlD => return Ok(false),
    }
    Ok(false)
}

fn setup_repl() -> ClapEditor<Command> {
    let prompt = DefaultPrompt {
        left_prompt: DefaultPromptSegment::Basic("ww_template".to_owned()),
        ..DefaultPrompt::default()
    };
    let rl = ClapEditor::<Command>::builder()
        .with_prompt(Box::new(prompt))
        .with_editor_hook(|reed| {
            // Do custom things with `Reedline` instance here
            reed.with_history(Box::new(
                FileBackedHistory::with_file(10000, "repl_history.txt".into()).unwrap(),
            ))
        })
        .build();
    rl
}
