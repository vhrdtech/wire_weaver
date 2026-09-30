//! Message transport over a byte stream medium whose IO is blocking and non-waking, e.g., RTT through
//! a debug probe (probe-rs is a blocking API), later UART.
//!
//! The medium is owned by a dedicated IO thread, that moves bytes between it and two channels:
//! chunks written by [StreamTx] go out, bytes read in are cut into messages by [StreamRx].
//! Stream boundaries are not preserved, so messages are never split (`write_full` in [ww_framer]),
//! same as `StreamTx` / `StreamRx` on the device side (`ww_device`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, trace, warn};
use ww_framer::traits::{Checksum, Head, Tail};

use crate::event_loop::transport::{MessageRx, MessageTx, Opened};

/// Byte stream medium, used from the IO thread only. Both calls must not wait for the other side.
pub(crate) trait BlockingStreamIo {
    /// Read whatever is available into `buf`, 0 if nothing.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, String>;
    /// Write as much as fits right now, 0 if nothing does.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, String>;
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct StreamConfig {
    /// Thread name, also used in error messages
    pub name: &'static str,
    /// Sleep when there was nothing to read or write
    pub poll_interval: Duration,
    /// Bytes read from the medium at once
    pub read_chunk: usize,
}

/// Chunks queued for the IO thread; small, so that a medium that does not take bytes pushes back
const TX_QUEUE: usize = 4;
/// Chunks read, but not yet taken by [StreamRx]
const RX_QUEUE: usize = 64;
/// How long [StreamTx::close] waits for queued chunks to be written out
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

/// Longest head, checksum and tail the framer adds to a message.
const fn overhead<H: Head, C: Checksum, T: Tail>() -> usize {
    H::MAX_HEAD_SIZE + C::LEN_BYTES_FULL + T::LEN_BYTES
}

pub(crate) type StreamOpened<H, C, T> = Opened<StreamTx<H, C, T>, StreamRx<H, C, T>>;

/// Start the IO thread and wait until `open` returns on it (e.g., attaching to a probe, which takes a
/// while), so that nothing is sent before the medium is usable.
/// Messages up to [DEFAULT_MAX_MESSAGE_SIZE](crate::DEFAULT_MAX_MESSAGE_SIZE) can be sent and received.
pub(crate) async fn open<I, H, C, T>(
    config: StreamConfig,
    open: impl FnOnce() -> Result<I, String> + Send + 'static,
) -> Result<StreamOpened<H, C, T>, String>
where
    I: BlockingStreamIo,
    H: Head<UserKind = u8>,
    C: Checksum,
    T: Tail,
{
    let (to_io, from_host) = mpsc::channel::<Vec<u8>>(TX_QUEUE);
    let (to_host, from_io) = mpsc::channel::<Result<Vec<u8>, String>>(RX_QUEUE);
    let (exited_tx, exited) = watch::channel(false);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let error = Arc::new(OnceLock::new());
    let thread_error = error.clone();
    let (opened_tx, opened_rx) = oneshot::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name(config.name.into())
        .spawn(move || {
            let mut from_host = from_host;
            // also signals a panicked thread
            let _exited = ExitedGuard(exited_tx);
            let io = match open() {
                Ok(io) => {
                    _ = opened_tx.send(Ok(()));
                    io
                }
                Err(e) => {
                    debug!("{}: open failed: {e}", config.name);
                    _ = opened_tx.send(Err(e));
                    return;
                }
            };
            if let Err(e) = io_thread(config, io, &mut from_host, &to_host, &thread_stop) {
                warn!("{}: {e}", config.name);
                // set before the channels close, so that whichever half notices first reports it
                _ = thread_error.set(e.clone());
                _ = to_host.blocking_send(Err(e));
            }
        })
        .map_err(|e| format!("{}: failed to spawn IO thread: {e}", config.name))?;
    opened_rx
        .await
        .map_err(|_| format!("{}: IO thread panicked while opening", config.name))??;
    let overhead = overhead::<H, C, T>();
    Ok(Opened {
        tx: StreamTx {
            framer: ww_framer::TxOwned::new(crate::DEFAULT_MAX_MESSAGE_SIZE + overhead),
            to_io: Some(to_io),
            stop,
            exited,
            error: error.clone(),
            name: config.name,
        },
        rx: StreamRx {
            framer: ww_framer::FramedRxOwned::new(
                crate::DEFAULT_MAX_MESSAGE_SIZE + overhead + config.read_chunk,
            ),
            from_io,
            chunk: vec![],
            chunk_pos: 0,
            error,
            name: config.name,
        },
    })
}

struct ExitedGuard(watch::Sender<bool>);

impl Drop for ExitedGuard {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

/// Runs until [StreamTx] is closed (and what it queued is written out) or dropped, [StreamTx::close]
/// times out, or the medium fails.
fn io_thread<I: BlockingStreamIo>(
    config: StreamConfig,
    mut io: I,
    from_host: &mut mpsc::Receiver<Vec<u8>>,
    to_host: &mpsc::Sender<Result<Vec<u8>, String>>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let name = config.name;
    debug!("{name}: IO thread started");
    let mut buf = vec![0u8; config.read_chunk];
    let mut chunk: Vec<u8> = vec![];
    let mut written = 0;
    let mut tx_closed = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let mut busy = false;
        if written == chunk.len() && !tx_closed {
            match from_host.try_recv() {
                Ok(c) => {
                    chunk = c;
                    written = 0;
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => tx_closed = true,
            }
        }
        if written < chunk.len() {
            let n = io.write(&chunk[written..])?;
            trace!("{name}: wrote {n}: {:02x?}", &chunk[written..written + n]);
            written += n;
            busy |= n > 0;
        } else if tx_closed {
            break;
        }
        let n = io.read(&mut buf)?;
        if n > 0 {
            trace!("{name}: read {n}: {:02x?}", &buf[..n]);
            busy = true;
            // rx half is gone: nothing reads replies anymore, keep writing until tx is closed too
            _ = to_host.blocking_send(Ok(buf[..n].to_vec()));
        }
        if !busy {
            std::thread::sleep(config.poll_interval);
        }
    }
    debug!("{name}: IO thread exited");
    Ok(())
}

/// Error the IO thread exited with, if it did
fn exit_error(error: &OnceLock<String>, name: &str) -> String {
    error
        .get()
        .cloned()
        .unwrap_or_else(|| format!("{name}: IO thread exited"))
}

/// [MessageTx] half, see [open].
pub(crate) struct StreamTx<H, C, T> {
    framer: ww_framer::TxOwned<H, C, T>,
    /// None once closed
    to_io: Option<mpsc::Sender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    exited: watch::Receiver<bool>,
    error: Arc<OnceLock<String>>,
    name: &'static str,
}

impl<H: Head<UserKind = u8>, C: Checksum, T: Tail> StreamTx<H, C, T> {
    async fn send_chunk(&mut self) -> Result<bool, String> {
        let Some(chunk) = self.framer.flush_to_vec() else {
            return Ok(false);
        };
        let to_io = self.to_io.as_ref().ok_or("closed")?;
        to_io
            .send(chunk)
            .await
            .map_err(|_| exit_error(&self.error, self.name))?;
        Ok(true)
    }
}

impl<H, C, T> MessageTx for StreamTx<H, C, T>
where
    H: Head<UserKind = u8> + Send + 'static,
    C: Checksum + Send + 'static,
    T: Tail + Send + 'static,
{
    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), String> {
        loop {
            match self.framer.write_full(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    // does not fit into what is left of the chunk: send and retry into an empty one
                    if !self.send_chunk().await? {
                        return Err("framing error: cannot progress".into());
                    }
                }
                Err(()) => {
                    return Err(format!(
                        "framing error: message too big ({})",
                        message.len()
                    ));
                }
            }
        }
    }

    async fn flush(&mut self) -> Result<(), String> {
        self.send_chunk().await.map(|_| ())
    }

    /// Lets the IO thread write out what is queued, then waits until it exits and releases the medium.
    async fn close(mut self) {
        self.to_io = None;
        let mut exited = self.exited.clone();
        if tokio::time::timeout(CLOSE_TIMEOUT, exited.wait_for(|e| *e))
            .await
            .is_err()
        {
            warn!("{}: queued bytes not written in time, dropping", self.name);
            self.stop.store(true, Ordering::Relaxed);
            _ = exited.wait_for(|e| *e).await;
        }
    }
}

impl<H, C, T> Drop for StreamTx<H, C, T> {
    fn drop(&mut self) {
        // not closed properly (e.g., a panic): do not wait for queued bytes
        if self.to_io.is_some() {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

/// [MessageRx] half, see [open]. Closing it is a no-op, the medium is released by [StreamTx::close].
pub(crate) struct StreamRx<H: Head, C, T> {
    framer: ww_framer::FramedRxOwned<H, C, T>,
    from_io: mpsc::Receiver<Result<Vec<u8>, String>>,
    /// Bytes from the IO thread that did not fit into the framer yet
    chunk: Vec<u8>,
    chunk_pos: usize,
    error: Arc<OnceLock<String>>,
    name: &'static str,
}

impl<H, C, T> MessageRx for StreamRx<H, C, T>
where
    H: Head<UserKind = u8> + Send + 'static,
    C: Checksum + Send + 'static,
    T: Tail + Send + 'static,
{
    /// Cancel-safe: bytes live in the framer and `chunk`, the only await is the channel receive.
    async fn read_message(&mut self) -> Result<(u8, &[u8]), String> {
        loop {
            // Consumes the message returned by the previous call (if any) and tries to cut the next one
            // out of what is already staged (a read can end anywhere in a message), before staging more.
            self.framer.reassemble();
            if self.framer.message().is_some() {
                break;
            }
            if self.chunk_pos == self.chunk.len() {
                self.chunk = self
                    .from_io
                    .recv()
                    .await
                    .ok_or_else(|| exit_error(&self.error, self.name))??;
                self.chunk_pos = 0;
            }
            if self.framer.free() == 0 {
                // only possible if the device sent a message larger than advertised
                warn!(
                    "{}: rx buffer overflow, dropping partial message",
                    self.name
                );
                self.framer.reset();
            }
            let len = self.framer.free().min(self.chunk.len() - self.chunk_pos);
            // cannot fail, at most free() bytes
            _ = self
                .framer
                .stage(&self.chunk[self.chunk_pos..self.chunk_pos + len]);
            self.chunk_pos += len;
        }
        // second lookup instead of returning from inside the loop: keeps the borrow checker happy
        Ok(self.framer.message().expect("checked above"))
    }
}
