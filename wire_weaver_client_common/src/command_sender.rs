use crate::timeout::Timeout;
use crate::{Command, Error};
use std::collections::HashMap;
use std::fmt::Debug;
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
pub struct CommandSender<F, E> {
    tx: UnboundedSender<Command<F, E>>,
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

// Resource path, intended to be used from generated client code in either of the two modes:
// * Absolute mode, then base_path is None and only Absolute variant will be accepted.
// * Relative mode, used to access trait resources without knowing anything else about an API, in which case base_path is Some.
//
// Differs from [ww_client_server::PathKind](ww_client_server::PathKind) in a subtle way, so that using only one type is not possible:
// in ww_client_server version all variants of this type essentially carry path (it's a separate field in [Request](ww_client_server::Request).
// here though, GlobalCompact and GlobalFull does not have a base_path, since generated client code cannot know it, hence the need for
// a separate type.
// pub enum CommandSenderPath<'i> {
//     Absolute {
//         path: &'i [u32],
//     },
//     GlobalCompact {
//         gid: CompactVersion,
//         path_from_trait: &'i [u32],
//     },
//     GlobalFull {
//         gid: FullVersion<'i>,
//         path_from_trait: &'i [u32],
//     },
// }

impl<F, E: Debug> CommandSender<F, E> {
    pub fn send(&self, command: Command<F, E>) -> Result<(), Error<E>> {
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
        timeout: Timeout,
    ) -> Result<Vec<u8>, Error<E>> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        let cmd = Command::SendCall {
            args_bytes: args,
            path_kind,
            timeout: Some(timeout.timeout()),
            done_tx: Some(done_tx),
        };
        let data = self
            .send_cmd_receive_reply(cmd, timeout.timeout(), done_rx, "call")
            .await?;
        Ok(data)
    }

    pub async fn send_write_receive_reply(
        &self,
        path: PathKind<'_>,
        value: Vec<u8>,
        timeout: Timeout,
    ) -> Result<(), Error<E>> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        let cmd = Command::SendWrite {
            value_bytes: value,
            path_kind,
            timeout: Some(timeout.timeout()),
            done_tx: Some(done_tx),
        };
        let _data = self
            .send_cmd_receive_reply(cmd, timeout.timeout(), done_rx, "write")
            .await?;
        Ok(())
    }

    pub async fn send_read_receive_reply(
        &self,
        path: PathKind<'_>,
        timeout: Timeout,
    ) -> Result<Vec<u8>, Error<E>> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        let cmd = Command::SendRead {
            path_kind,
            timeout: Some(timeout.timeout()),
            done_tx: Some(done_tx),
        };
        let data = self
            .send_cmd_receive_reply(cmd, timeout.timeout(), done_rx, "read")
            .await?;
        Ok(data)
    }

    async fn send_cmd_receive_reply(
        &self,
        cmd: Command<F, E>,
        timeout: Duration,
        done_rx: oneshot::Receiver<Result<Vec<u8>, Error<E>>>,
        desc: &'static str,
    ) -> Result<Vec<u8>, Error<E>> {
        self.tx.send(cmd).map_err(|_| Error::EventLoopNotRunning)?;
        let rx_or_timeout = tokio::time::timeout(timeout, done_rx).await;
        trace!("got {desc} response: {:02x?}", rx_or_timeout);
        let rx_or_recv_err = rx_or_timeout.map_err(|_| Error::Timeout)?;
        let response = rx_or_recv_err.map_err(|_| Error::EventLoopNotRunning)?;
        let data = response?;
        Ok(data)
    }

    fn to_ww_client_server_path(&self, path: PathKind<'_>) -> Result<PathKindOwned, Error<E>> {
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
                        gid: compact.clone(),
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
