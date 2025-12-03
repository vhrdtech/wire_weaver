use crate::rx_dispatcher::{DispatcherCommand, DispatcherMessage};
use crate::{Command, DeviceFilter, Error, OnError, SeqTy, StreamEvent};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::trace;
use wire_weaver::prelude::UNib32;
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use ww_client_server::{PathKind, PathKindOwned, RequestKindOwned, StreamSidebandCommand};
use ww_version::{CompactVersion, FullVersionOwned};

/// Entry point for an API root or API trait implementation. Inside - wrapper over a channel sender half (currently tokio::mpsc::UnboundedSender).
///
/// Commands sent through this channel are received by a worker thread (e.g., USB or WebSocket clients) and forwarded to a connected device.
/// Replies are received through one-shot channels created on the fly when requests are sent.
pub struct CommandSender {
    transport_cmd_tx: mpsc::UnboundedSender<Command>,
    dispatcher_cmd_tx: mpsc::UnboundedSender<DispatcherCommand>,
    seq_rx: mpsc::Receiver<SeqTy>,
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
    scratch: [u8; 4096],
}

impl CommandSender {
    pub fn new(
        transport_cmd_tx: mpsc::UnboundedSender<Command>,
        dispatcher_msg_rx: mpsc::UnboundedReceiver<DispatcherMessage>,
    ) -> Self {
        let (seq_tx, seq_rx) = mpsc::channel(16); // TODO: increase to 1024?
        let (dispatcher_cmd_tx, dispatcher_cmd_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            crate::rx_dispatcher::rx_dispatcher(dispatcher_cmd_rx, dispatcher_msg_rx).await;
        });
        _ = dispatcher_cmd_tx.send(DispatcherCommand::RegisterSeqSource { seq_tx });
        Self {
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx,
            trait_path: None,
            gid_map: HashMap::new(),
            scratch: [0; 4096],
        }
    }

    pub async fn connect(
        &mut self,
        filter: DeviceFilter,
        user_protocol_version: FullVersionOwned,
        on_error: OnError,
    ) -> Result<(), Error> {
        let (connected_tx, connected_rx) = oneshot::channel();
        self.transport_cmd_tx
            .send(Command::Connect {
                filter,
                user_protocol_version,
                on_error,
                connected_tx: Some(connected_tx),
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        let connection_result = connected_rx.await.map_err(|_| Error::EventLoopNotRunning)?;
        connection_result?;
        Ok(())
    }

    pub fn send(&self, command: Command) -> Result<(), Error> {
        // TODO: Add command tx limit?
        self.transport_cmd_tx
            .send(command)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Sends call request to remote device, awaits response and returns it.
    pub async fn send_call_receive_reply(
        &mut self,
        path: PathKind<'_>,
        args: Vec<u8>,
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        // obtain next seq and correct path
        let seq = self.next_seq().await?;
        let path_kind = self.to_ww_client_server_path(path)?;

        // notify rx dispatcher
        let (done_tx, done_rx) = oneshot::channel();
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnCallReturn {
                seq,
                done_tx,
                timeout: Some(timeout),
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;

        // send call command to remote device
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::Call { args },
        };
        let req = req.to_ww_bytes(&mut self.scratch)?; // TODO: recycle Vec (serialize via BufWriter with Vec flavor)
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };

        // await return value from remote device (routed through rx dispatcher)
        let data = self
            .send_cmd_receive_reply(cmd, timeout, done_rx, "call")
            .await?;
        Ok(data)
    }

    /// Sends call request to remote device with seq == 0.
    /// No response will be generated by remote device, nor it will be awaited.
    pub async fn send_call_forget(
        &mut self,
        path: PathKind<'_>,
        args: Vec<u8>,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Call { args },
        };
        let req = req.to_ww_bytes(&mut self.scratch)?;
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };
        self.transport_cmd_tx
            .send(cmd)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Sends write request to remote device, awaits response and returns.
    pub async fn send_write_receive_reply(
        &mut self,
        path: PathKind<'_>,
        value_bytes: Vec<u8>,
        timeout: Duration,
    ) -> Result<(), Error> {
        let seq = self.next_seq().await?;
        let path_kind = self.to_ww_client_server_path(path)?;
        let (done_tx, done_rx) = oneshot::channel();
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnWriteComplete {
                seq,
                done_tx,
                timeout: Some(timeout),
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::Write { data: value_bytes },
        };
        let req = req.to_ww_bytes(&mut self.scratch)?;
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };
        let _data = self
            .send_cmd_receive_reply(cmd, timeout, done_rx, "write")
            .await?;
        Ok(())
    }

    /// Sends write request to remote device with seq == 0.
    /// No response will be generated by remote device, nor it will be awaited.
    pub fn send_write_forget(
        &mut self,
        path: PathKind<'_>,
        value_bytes: Vec<u8>,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::Write { data: value_bytes },
        };
        let req = req.to_ww_bytes(&mut self.scratch)?;
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };
        self.transport_cmd_tx
            .send(cmd)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Sends open sideband command to remote device and setups subscribes to stream events.
    pub fn send_stream_open(
        &mut self,
        path: PathKind<'_>,
        stream_event_tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<(), Error> {
        let path_kind = self.to_ww_client_server_path(path)?;

        // notify rx dispatcher
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnStreamEvent {
                path_kind: path_kind.clone(),
                stream_event_tx,
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;

        // send sideband command to remote device
        let req = ww_client_server::RequestOwned {
            seq: 0,
            path_kind,
            kind: RequestKindOwned::StreamSideband {
                sideband_cmd: StreamSidebandCommand::Open,
            },
        };
        let req = req.to_ww_bytes(&mut self.scratch)?;
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };
        self.transport_cmd_tx
            .send(cmd)
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Sends read request to remote device, awaits response and returns it.
    pub async fn send_read_receive_reply(
        &mut self,
        path: PathKind<'_>,
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        // obtain next seq and correct path
        let seq = self.next_seq().await?;
        let path_kind = self.to_ww_client_server_path(path)?;

        // notify rx dispatcher
        let (done_tx, done_rx) = oneshot::channel();
        self.dispatcher_cmd_tx
            .send(DispatcherCommand::OnReadValue {
                seq,
                done_tx,
                timeout: Some(timeout),
            })
            .map_err(|_| Error::RxDispatcherNotRunning)?;

        // send read command to remote device
        let req = ww_client_server::RequestOwned {
            seq,
            path_kind,
            kind: RequestKindOwned::Read,
        };
        let req = req.to_ww_bytes(&mut self.scratch)?;
        let cmd = Command::SendMessage {
            bytes: req.to_vec(),
        };

        // await response from remote device (through rx dispatcher)
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
        self.transport_cmd_tx
            .send(cmd)
            .map_err(|_| Error::EventLoopNotRunning)?;
        let rx_or_timeout = tokio::time::timeout(timeout, done_rx).await;
        trace!("got {desc} response: {:02x?}", rx_or_timeout);
        let rx_or_recv_err = rx_or_timeout.map_err(|_| Error::Timeout)?;
        let response = rx_or_recv_err.map_err(|_| Error::EventLoopNotRunning)?;
        let data = response?;
        Ok(data)
    }

    async fn next_seq(&mut self) -> Result<SeqTy, Error> {
        let seq = self
            .seq_rx
            .recv()
            .await
            .ok_or(Error::RxDispatcherNotRunning)?;
        Ok(seq)
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
