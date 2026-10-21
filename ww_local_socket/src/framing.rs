use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Reads `| len: u32 LE | kind: u8 | payload |` messages. Cancel-safe: a partly read message stays in `self`.
pub struct MessageReader<R> {
    r: R,
    buf: Vec<u8>,
    /// Bytes of the current message (length included) read so far
    have: usize,
    /// Longest `kind + payload` accepted
    max: usize,
    /// A whole message is in `buf`, until consumed
    done: bool,
}

impl<R: AsyncRead + Unpin> MessageReader<R> {
    /// `max_message_len` is the longest payload accepted (the kind byte comes on top); a longer one is an error and
    /// the connection is to be dropped, the stream can't be re-synchronized.
    pub fn new(r: R, max_message_len: usize) -> Self {
        MessageReader {
            r,
            buf: vec![0u8; 4 + 1 + max_message_len],
            have: 0,
            max: max_message_len + 1,
            done: false,
        }
    }

    /// Next message, borrowed until the next call. EOF between messages is `UnexpectedEof` too: a peer that goes
    /// away mid-message and one that closes cleanly both end the link.
    pub async fn read_message(&mut self) -> io::Result<(u8, &[u8])> {
        self.consume();
        self.wait_message().await?;
        Ok(self.message().expect("a message was just received"))
    }

    /// Waits until [message](Self::message) returns one; returns at once while the current one isn't
    /// [consumed](Self::consume). The split lets a caller wait in a `select` and borrow the message afterwards.
    pub async fn wait_message(&mut self) -> io::Result<()> {
        if self.done {
            return Ok(());
        }
        while self.have < 4 {
            self.fill(4).await?;
        }
        let len = self.len();
        if len == 0 || len > self.max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("message of {len} bytes, at most {} accepted", self.max),
            ));
        }
        while self.have < 4 + len {
            self.fill(4 + len).await?;
        }
        self.done = true;
        Ok(())
    }

    /// The message [wait_message](Self::wait_message) received, until [consume](Self::consume).
    pub fn message(&self) -> Option<(u8, &[u8])> {
        self.done
            .then(|| (self.buf[4], &self.buf[5..4 + self.len()]))
    }

    /// Done with the current message, the next wait reads the next one.
    pub fn consume(&mut self) {
        if self.done {
            self.have = 0;
            self.done = false;
        }
    }

    fn len(&self) -> usize {
        u32::from_le_bytes(self.buf[..4].try_into().unwrap()) as usize
    }

    async fn fill(&mut self, upto: usize) -> io::Result<()> {
        // one read per await: dropping the future loses nothing, the bytes are in buf already
        let n = self.r.read(&mut self.buf[self.have..upto]).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.have += n;
        Ok(())
    }

    pub fn into_inner(self) -> R {
        self.r
    }
}

/// Writes `| len: u32 LE | kind: u8 | payload |` messages, held back until [flush](Self::flush), so the messages
/// of one event loop turn go out in one write. Cancel-safe: what is already written is not written again.
pub struct MessageWriter<W> {
    w: W,
    buf: Vec<u8>,
    /// Bytes of `buf` already written
    sent: usize,
}

impl<W: AsyncWrite + Unpin> MessageWriter<W> {
    pub fn new(w: W) -> Self {
        MessageWriter {
            w,
            buf: Vec::with_capacity(4096),
            sent: 0,
        }
    }

    /// Queues a message, nothing is written until [flush](Self::flush).
    pub fn push(&mut self, kind: u8, payload: &[u8]) {
        if self.sent == self.buf.len() {
            self.buf.clear();
            self.sent = 0;
        }
        self.buf
            .extend_from_slice(&(1 + payload.len() as u32).to_le_bytes());
        self.buf.push(kind);
        self.buf.extend_from_slice(payload);
    }

    /// Writes everything queued.
    pub async fn flush(&mut self) -> io::Result<()> {
        while self.sent < self.buf.len() {
            let n = self.w.write(&self.buf[self.sent..]).await?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            self.sent += n;
        }
        self.buf.clear();
        self.sent = 0;
        self.w.flush().await
    }

    /// Drops what is queued and not yet written. A message partly written stays, so the stream stays in step.
    pub fn clear(&mut self) {
        if self.sent == 0 {
            self.buf.clear();
            return;
        }
        let mut end = 0;
        // keep whole messages up to and including the one being written
        while end < self.sent {
            let len = u32::from_le_bytes(self.buf[end..end + 4].try_into().unwrap()) as usize;
            end += 4 + len;
        }
        self.buf.truncate(end);
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.w.shutdown().await
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_split_reads() {
        let (a, b) = tokio::io::duplex(3); // tiny pipe: every message arrives in pieces
        let mut w = MessageWriter::new(a);
        let mut r = MessageReader::new(b, 64);
        let writer = tokio::spawn(async move {
            w.push(1, b"hello");
            w.push(2, b"");
            w.flush().await.unwrap();
            w.push(3, &[7u8; 64]);
            w.flush().await.unwrap();
        });
        assert_eq!(r.read_message().await.unwrap(), (1, &b"hello"[..]));
        assert_eq!(r.read_message().await.unwrap(), (2, &b""[..]));
        assert_eq!(r.read_message().await.unwrap(), (3, &[7u8; 64][..]));
        writer.await.unwrap();
        let e = r.read_message().await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn too_long_and_empty_refused() {
        let bytes = [100u8, 0, 0, 0, 1];
        let mut r = MessageReader::new(&bytes[..], 64);
        assert_eq!(
            r.read_message().await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let bytes = [0u8, 0, 0, 0];
        let mut r = MessageReader::new(&bytes[..], 64);
        assert_eq!(
            r.read_message().await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn read_is_cancel_safe() {
        let (mut a, b) = tokio::io::duplex(64);
        let mut r = MessageReader::new(b, 64);
        a.write_all(&[6, 0, 0, 0, 9, b'a']).await.unwrap();
        // the rest is not there yet: the read is cancelled with half a message in the reader
        let t = tokio::time::timeout(std::time::Duration::from_millis(10), r.read_message()).await;
        assert!(t.is_err());
        a.write_all(b"bcde").await.unwrap();
        assert_eq!(r.read_message().await.unwrap(), (9, &b"abcde"[..]));
    }

    #[tokio::test]
    async fn clear_keeps_a_message_being_written() {
        let (a, b) = tokio::io::duplex(6);
        let mut w = MessageWriter::new(a);
        w.push(1, b"abcdef");
        w.push(2, b"x");
        // the pipe takes 6 bytes: the first message is half written when the flush is cancelled
        let _ = tokio::time::timeout(std::time::Duration::from_millis(10), w.flush()).await;
        w.clear();
        w.push(3, b"y");
        let mut r = MessageReader::new(b, 64);
        let reader = tokio::spawn(async move {
            assert_eq!(r.read_message().await.unwrap(), (1, &b"abcdef"[..]));
            assert_eq!(r.read_message().await.unwrap(), (3, &b"y"[..]));
        });
        w.flush().await.unwrap();
        reader.await.unwrap();
    }
}
