use crate::{Command, Error, StreamEvent};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use tracing::trace;
use wire_weaver::prelude::UNib32;
use ww_client_server::{PathKind, PathKindOwned};
use ww_version::{CompactVersion, FullVersionOwned};

/// Entry point for an API root or API trait implementation. Inside - wrapper over a channel sender half (currently tokio::mpsc::UnboundedSender).
///
/// Commands sent through this channel are received by a worker thread (e.g., USB or WebSocket clients) and forwarded to a connected device.
/// Replies are received through one-shot channels created on the fly when requests are sent.
pub struct CommandSender {
    tx: UnboundedSender<Command>,
    // TODO: CommandSender outstanding request limit?
    /// * None for command sender attached to API root, trait addressing will result in an error.
    /// * Some(empty path) for trait implemented at root level (unknown path), trait addressing will be used.
    /// * Some(non-empty path) for trait implemented at some particular path, trait addressing will be substituted with an actual path.
    ///
    /// "Some" cases are only for trait clients (ww_impl! macro). API root client (ww_api!) uses absolute addressing.
    trait_path: Option<Vec<UNib32>>,

    /// Mapping from FullVersion (crate name as string + semver) to trait GID of much smaller size.
    /// Optimally, should be requested from a device, so that newly assigned GIDs not yet known to a device are not used.
    /// But can also be forced to a known GID if performance is critical.
    gid_map: HashMap<FullVersionOwned, CompactVersion>,
}

impl CommandSender {
    pub fn new(tx: UnboundedSender<Command>) -> Self {
        Self {
            tx,
            trait_path: None,
            gid_map: HashMap::new(),
        }
    }

    pub fn send(&self, command: Command) -> Result<(), Error> {
        // TODO: Add command tx limit?
        self.tx
            .send(command)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub async fn send_call_receive_reply(
        &self,
        path: PathKind<'_>,
        args: Vec<u8>,
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        let cmd = Command::SendCall {
            args_bytes: args,
            path_kind,
            timeout: Some(timeout),
            done_tx: Some(done_tx),
        };
        let data = self
            .send_cmd_receive_reply(cmd, timeout, done_rx, "call")
            .await?;
        Ok(data)
    }

    pub async fn send_call_forget(&self, path: PathKind<'_>, args: Vec<u8>) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let cmd = Command::SendCall {
            args_bytes: args,
            path_kind,
            timeout: None,
            done_tx: None,
        };
        self.tx.send(cmd).map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub async fn send_write_receive_reply(
        &self,
        path: PathKind<'_>,
        value_bytes: Vec<u8>,
        timeout: Duration,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        let cmd = Command::SendWrite {
            path_kind,
            value_bytes,
            timeout: Some(timeout),
            done_tx: Some(done_tx),
        };
        let _data = self
            .send_cmd_receive_reply(cmd, timeout, done_rx, "write")
            .await?;
        Ok(())
    }

    pub fn send_write_forget(&self, path: PathKind<'_>, value_bytes: Vec<u8>) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let cmd = Command::SendWrite {
            path_kind,
            value_bytes,
            timeout: None,
            done_tx: None,
        };
        self.tx.send(cmd).map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub fn send_stream_open(
        &self,
        path: PathKind<'_>,
        stream_data_tx: UnboundedSender<StreamEvent>,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let cmd = Command::StreamOpen {
            path_kind,
            stream_data_tx,
        };
        self.tx.send(cmd).map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub async fn send_read_receive_reply(
        &self,
        path: PathKind<'_>,
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        let cmd = Command::SendRead {
            path_kind,
            timeout: Some(timeout),
            done_tx: Some(done_tx),
        };
        let data = self
            .send_cmd_receive_reply(cmd, timeout, done_rx, "read")
            .await?;
        Ok(data)
    }

    async fn send_cmd_receive_reply(
        &self,
        cmd: Command,
        timeout: Duration,
        done_rx: oneshot::Receiver<Result<Vec<u8>, Error>>,
        desc: &'static str,
    ) -> Result<Vec<u8>, Error> {
        self.tx.send(cmd).map_err(|_| Error::EventLoopNotRunning)?;
        let rx_or_timeout = tokio::time::timeout(timeout, done_rx).await;
        trace!("got {desc} response: {:02x?}", rx_or_timeout);
        let rx_or_recv_err = rx_or_timeout.map_err(|_| Error::Timeout)?;
        let response = rx_or_recv_err.map_err(|_| Error::EventLoopNotRunning)?;
        let data = response?;
        Ok(data)
    }

    fn to_ww_client_server_path(&self, path: PathKind<'_>) -> Result<PathKindOwned, Error> {
        if matches!(path, PathKind::Absolute { .. }) && self.trait_path.is_some() {
            return Err(Error::User(
                "CommandSender configured as trait attachment, but used with absolute path".into(),
            ));
        }
        let path_kind = match path {
            PathKind::Absolute { path } => PathKindOwned::Absolute {
                path: path.iter().collect::<Result<Vec<_>, _>>()?,
            },
            PathKind::GlobalCompact {
                gid,
                path_from_trait,
            } => {
                if let Some(base) = &self.trait_path {
                    let mut path = base.clone();
                    for n in path_from_trait.iter() {
                        path.push(n?);
                    }
                    PathKindOwned::Absolute { path }
                } else {
                    PathKindOwned::GlobalCompact {
                        gid,
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                }
            }
            PathKind::GlobalFull {
                gid,
                path_from_trait,
            } => {
                if let Some(base) = &self.trait_path {
                    let mut path = base.clone();
                    for n in path_from_trait.iter() {
                        path.push(n?);
                    }
                    PathKindOwned::Absolute { path }
                } else if let Some(compact) = self.gid_map.get(&gid.make_owned()) {
                    // TODO: actually not possible to implement Borrow for FullVersionOwned?
                    PathKindOwned::GlobalCompact {
                        gid: *compact,
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                } else {
                    PathKindOwned::GlobalFull {
                        gid: gid.make_owned(),
                        path_from_trait: path_from_trait.iter().collect::<Result<Vec<_>, _>>()?,
                    }
                }
            }
        };
        Ok(path_kind)
    }
}
