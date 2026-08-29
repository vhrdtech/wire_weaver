use std::marker::PhantomData;

#[cfg(feature = "usb")]
use crate::config::InterfaceKind;
use crate::{Commander, WwClient, config::ClientConfig, event_loop::command::Command};
use anyhow::{Error, Result, bail};
use tokio::sync::{mpsc, oneshot};

pub struct PreparedConnection<T> {
    config: ClientConfig,
    _phantom: PhantomData<T>,
}

impl<T: WwClient> PreparedConnection<T> {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            _phantom: PhantomData,
        }
    }

    pub async fn connect(self) -> Result<T> {
        let config = self.config.validate()?;

        let (cmd_tx, cmd_rx) = mpsc::channel(config.cmd_queue_size);
        let mut cmd_rx = Some(cmd_rx);
        let mut handle = None;
        for iface_kind in config.interfaces() {
            #[cfg(feature = "usb")]
            if iface_kind == InterfaceKind::Usb {
                match crate::usb::try_connect(&config, &mut cmd_rx).await {
                    Ok(Some(h)) => {
                        handle = Some(h);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => bail_on_ambiguous(e)?,
                }
            }
        }

        let (connected_tx, connected_rx) = oneshot::channel();
        if let Some(handle) = handle {
            cmd_tx
                .send(Command::Connect {
                    handle,
                    client_version: config.client_version.clone(),
                    connected_tx: Some(connected_tx),
                    failed_tx: None,
                })
                .await
                .map_err(|_| crate::Error::EventLoopNotRunning)?;
        }
        let info = connected_rx.await?;
        let device_api_info = info.result?;

        let mut commander = Commander::new(cmd_tx);
        commander.set_local_timeout(config.default_timeout);
        commander.connected_device = device_api_info;
        if let Some((api_bundle, signature)) = config.introspect {
            commander.set_introspect_data(api_bundle, signature);
        }
        Ok(T::from_cmd(commander))
    }

    pub fn connect_blocking(self) -> Result<()> {
        todo!()
    }

    pub fn connect_promise(self) {
        todo!()
    }

    /// Try to connect to a selected device immediately, but if it fails or there are no devices, keep trying.
    /// Even when connected and then disconnected or errored out, try again, until commanded to stop and exit.
    /// Commander and all open streams will be kept active.
    pub fn connect_keep_trying(self) {
        // figure out how to handle Commander.connected_device if connected to a different device, reject by serial?
        todo!()
    }
}

fn bail_on_ambiguous(e: Error) -> Result<()> {
    if let Some(e) = e.downcast_ref::<crate::Error>() {
        if let crate::Error::AmbiguousDeviceChoice(devices) = e {
            println!("Matched devices:");
            for dev in devices {
                println!("{dev:?}");
            }
            bail!("Ambiguous device choice");
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}
