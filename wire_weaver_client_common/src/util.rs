use crate::{Command, Error, Timeout};
use std::fmt::Debug;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::trace;
use wire_weaver::shrink_wrap::nib32::UNib32;

#[cfg(not(any(
    feature = "default-timeout-100ms",
    feature = "default-timeout-250ms",
    feature = "default-timeout-1s"
)))]
compile_error!("Select one of the default-timeout-x features");

#[cfg(all(
    feature = "default-timeout-100ms",
    any(feature = "default-timeout-250ms", feature = "default-timeout-1s")
))]
compile_error!("Select only one of the default-timeout-x features");
#[cfg(all(
    feature = "default-timeout-250ms",
    any(feature = "default-timeout-100ms", feature = "default-timeout-1s")
))]
compile_error!("Select only one of the default-timeout-x features");
#[cfg(all(
    feature = "default-timeout-1s",
    any(feature = "default-timeout-100ms", feature = "default-timeout-250ms")
))]
compile_error!("Select only one of the default-timeout-x features");

#[cfg(feature = "default-timeout-100ms")]
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);
#[cfg(feature = "default-timeout-250ms")]
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(feature = "default-timeout-1s")]
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1_000);

impl Timeout {
    fn timeout(&self) -> Duration {
        match self {
            Timeout::Default => DEFAULT_TIMEOUT,
            Timeout::Millis(millis) => Duration::from_millis(*millis),
        }
    }
}

pub async fn send_call_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    args: Vec<u8>,
    path: &[u32],
    timeout: Timeout,
) -> Result<Vec<u8>, Error<E>> {
    let (done_tx, done_rx) = oneshot::channel();
    let cmd = Command::SendCall {
        args_bytes: args,
        path: path.iter().map(|i| UNib32(*i)).collect(),
        timeout: Some(timeout.timeout()),
        done_tx: Some(done_tx),
    };
    let data = send_cmd_receive_reply(cmd_tx, cmd, timeout.timeout(), done_rx, "call").await?;
    Ok(data)
}

pub async fn send_write_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    value: Vec<u8>,
    path: &[u32],
    timeout: Timeout,
) -> Result<(), Error<E>> {
    let (done_tx, done_rx) = oneshot::channel();
    let cmd = Command::SendWrite {
        value_bytes: value,
        path: path.iter().map(|i| UNib32(*i)).collect(),
        timeout: Some(timeout.timeout()),
        done_tx: Some(done_tx),
    };
    let _data = send_cmd_receive_reply(cmd_tx, cmd, timeout.timeout(), done_rx, "write").await?;
    Ok(())
}

pub async fn send_read_receive_reply<F, E: Debug>(
    cmd_tx: &mut mpsc::UnboundedSender<Command<F, E>>,
    path: &[u32],
    timeout: Timeout,
) -> Result<Vec<u8>, Error<E>> {
    let (done_tx, done_rx) = oneshot::channel();
    let cmd = Command::SendRead {
        path: path.iter().map(|i| UNib32(*i)).collect(),
        timeout: Some(timeout.timeout()),
        done_tx: Some(done_tx),
    };
    let data = send_cmd_receive_reply(cmd_tx, cmd, timeout.timeout(), done_rx, "read").await?;
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
