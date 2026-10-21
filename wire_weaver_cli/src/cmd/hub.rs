//! `ww hub`: the per-user hub that holds devices and serves every program (SRV-13). `serve` runs it in the
//! foreground; status, stop, who, take, keep and lend come with TOOL-17.

use std::time::Duration;

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum HubCommand {
    /// Run the hub in the foreground until it is idle or stopped (programs start it by themselves when it pays off;
    /// this is for watching its log or debugging)
    Serve {
        /// Exit after this many seconds without connections (default 300, or WW_HUB_IDLE_EXIT)
        #[arg(long)]
        idle_exit: Option<u64>,
    },
}

pub(crate) async fn hub(cmd: HubCommand) -> Result<()> {
    match cmd {
        HubCommand::Serve { idle_exit } => {
            let mut config = ww_hub::HubConfig::from_env()?;
            if let Some(s) = idle_exit {
                config.idle_exit = Duration::from_secs(s);
            }
            match ww_hub::serve(config).await? {
                ww_hub::Exit::AlreadyRunning => {
                    anyhow::bail!("a hub runs for this user already")
                }
                exit => eprintln!("ww hub: {exit:?}"),
            }
        }
    }
    Ok(())
}
