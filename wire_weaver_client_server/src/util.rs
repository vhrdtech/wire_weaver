use crate::{Command, Error};
use std::fmt::Debug;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::trace;
use wire_weaver::shrink_wrap::nib16::Nib16;

pub async fn send_call_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    args: Vec<u8>,
    path: Vec<Nib16>,
) -> Result<Vec<u8>, Error<E>> {
    let (done_tx, done_rx) = oneshot::channel();
    let timeout = Duration::from_millis(250);
    let cmd = Command::SendCall {
        args_bytes: args,
        path,
        timeout: Some(timeout),
        done_tx: Some(done_tx),
    };
    let data = send_cmd_receive_reply(cmd_tx, cmd, timeout, done_rx, "call").await?;
    Ok(data)
}

pub async fn send_write_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    value: Vec<u8>,
    path: Vec<Nib16>,
) -> Result<(), Error<E>> {
    let (done_tx, done_rx) = oneshot::channel();
    let timeout = Duration::from_millis(250);
    let cmd = Command::SendWrite {
        value_bytes: value,
        path,
        timeout: Some(timeout),
        done_tx: Some(done_tx),
    };
    let _data = send_cmd_receive_reply(cmd_tx, cmd, timeout, done_rx, "write").await?;
    Ok(())
}

pub async fn send_read_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    path: Vec<Nib16>,
) -> Result<Vec<u8>, Error<E>> {
    let (done_tx, done_rx) = oneshot::channel();
    let timeout = Duration::from_millis(250);
    let cmd = Command::SendRead {
        path,
        timeout: Some(timeout),
        done_tx: Some(done_tx),
    };
    let data = send_cmd_receive_reply(cmd_tx, cmd, timeout, done_rx, "read").await?;
    Ok(data)
}

async fn send_cmd_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    cmd: Command<F, E>,
    timeout: Duration,
    done_rx: oneshot::Receiver<Result<Vec<u8>, Error<E>>>,
    desc: &'static str,
) -> Result<Vec<u8>, Error<E>> {
    cmd_tx.send(cmd).map_err(|_| Error::EventLoopNotRunning)?;
    let rx_or_timeout = tokio::time::timeout(timeout, done_rx).await;
    trace!("got {desc} response: {:02x?}", rx_or_timeout);
    let rx_or_recv_err = rx_or_timeout.map_err(|_| Error::Timeout)?;
    let response = rx_or_recv_err.map_err(|_| Error::EventLoopNotRunning)?;
    let data = response?;
    Ok(data)
}
