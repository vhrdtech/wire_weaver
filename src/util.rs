use crate::{Command, Error};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::trace;
use wire_weaver::shrink_wrap::nib16::Nib16;

pub async fn send_call_receive_reply(
    cmd_tx: &mut mpsc::UnboundedSender<Command>,
    args: Vec<u8>,
    path: Vec<Nib16>,
) -> Result<Vec<u8>, Error> {
    let (done_tx, done_rx) = oneshot::channel();
    let timeout = Duration::from_secs(250);
    cmd_tx
        .send(Command::SendCall {
            args_bytes: args,
            path,
            timeout: Some(timeout),
            done_tx: Some(done_tx),
        })
        .map_err(|_| Error::EventLoopNotRunning)?;
    let rx_or_timeout = tokio::time::timeout(timeout, done_rx).await;
    trace!("got call response: {:02x?}", rx_or_timeout);
    let rx_or_recv_err = rx_or_timeout.map_err(|_| Error::Timeout)?;
    let response = rx_or_recv_err.map_err(|_| Error::EventLoopNotRunning)?;
    let data = response?;
    Ok(data)
}
