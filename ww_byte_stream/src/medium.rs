//! One connection on a byte stream as a `ww_device` medium: what a server runs a `DeviceLink` on once the preamble
//! was answered. Messages are framed as [MessageWriter] and [MessageReader] do; a reliable stream needs no
//! `ww_framer`.
//!
//! The framing is length-prefixed and trusts the stream: one lost or flipped byte puts the reader out of step for
//! good, which ends the connection. A lossy stream (a UART, a radio) needs resynchronisation and a checksum in its own
//! framing, a medium beside this one.

use std::io;

use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadHalf, WriteHalf};

use crate::{MessageReader, MessageWriter};

/// The message halves of one connection on `stream`, taking messages of up to `message_len_max` bytes.
pub fn medium<S: AsyncRead + AsyncWrite>(
    stream: S,
    message_len_max: usize,
) -> (StreamTx<WriteHalf<S>>, StreamRx<ReadHalf<S>>) {
    let (r, w) = stream.split();
    (
        StreamTx {
            writer: Some(MessageWriter::new(w)),
        },
        StreamRx {
            reader: Some(MessageReader::new(r, message_len_max)),
            message_len_max,
        },
    )
}

/// Sends the messages of one connection; down for good after a write failed.
pub struct StreamTx<W> {
    writer: Option<MessageWriter<W>>,
}

/// Receives the messages of one connection; down for good after the stream ended or went out of step.
pub struct StreamRx<R> {
    reader: Option<MessageReader<R>>,
    message_len_max: usize,
}

/// The connection is down: its stream failed or ended before.
fn down() -> io::Error {
    io::ErrorKind::NotConnected.into()
}

impl<W: AsyncWrite + Unpin> ww_device::MessageTx for StreamTx<W> {
    type Error = io::Error;

    async fn write_message(&mut self, kind: u8, message: &[u8]) -> io::Result<()> {
        self.writer.as_mut().ok_or_else(down)?.push(kind, message);
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()> {
        let w = self.writer.as_mut().ok_or_else(down)?;
        let r = w.flush().await;
        if r.is_err() {
            self.writer = None;
        }
        r
    }

    /// Drops what is held back; a message partly written stays, so the stream stays in step.
    fn reset(&mut self) {
        if let Some(w) = self.writer.as_mut() {
            w.clear();
        }
    }
}

impl<R: AsyncRead + Unpin> ww_device::MessageRx for StreamRx<R> {
    type Error = io::Error;

    async fn wait_message(&mut self) -> io::Result<()> {
        let r = self.reader.as_mut().ok_or_else(down)?;
        let result = r.wait_message().await;
        if result.is_err() {
            self.reader = None;
        }
        result
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        self.reader.as_ref()?.message()
    }

    fn consume(&mut self) {
        if let Some(r) = self.reader.as_mut() {
            r.consume();
        }
    }

    /// Ready at once while the stream is up; a connection that ended never comes back.
    async fn wait_connected(&mut self) {
        if self.reader.is_none() {
            std::future::pending::<()>().await;
        }
    }

    /// Nothing partial to drop on a stream in step: a message partly read stays for the next wait.
    fn reset(&mut self) {
        self.consume();
    }

    fn max_message_len(&self) -> usize {
        self.message_len_max
    }
}
