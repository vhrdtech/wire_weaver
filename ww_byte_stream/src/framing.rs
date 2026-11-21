use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Reads `| len: u32 LE | kind: u8 | payload |` messages. Each read takes whatever the peer wrote (a batch of
/// messages is one read, not two per message), and the messages are taken from the buffer one by one. Cancel-safe:
/// what was read stays in `self`.
pub struct MessageReader<R> {
    r: R,
    buf: Vec<u8>,
    /// Unread bytes are `buf[start..end]`, the current message starts at `start`
    start: usize,
    end: usize,
    /// Longest `kind + payload` accepted
    max: usize,
    /// A whole message is at `start`, until consumed
    done: bool,
}

/// Bytes a read can take at least, so a batch of small messages comes in with one read.
const READ_AHEAD: usize = 16 * 1024;

impl<R: AsyncRead + Unpin> MessageReader<R> {
    /// `max_message_len` is the longest payload accepted (the kind byte comes on top); a longer one is an error and
    /// the connection is to be dropped, the stream can't be re-synchronized.
    pub fn new(r: R, max_message_len: usize) -> Self {
        MessageReader {
            r,
            buf: vec![0u8; (4 + 1 + max_message_len).max(READ_AHEAD)],
            start: 0,
            end: 0,
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
        loop {
            let have = self.end - self.start;
            if have >= 4 {
                let len = self.len();
                if len == 0 || len > self.max {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("message of {len} bytes, at most {} accepted", self.max),
                    ));
                }
                if have >= 4 + len {
                    self.done = true;
                    return Ok(());
                }
            }
            self.fill().await?;
        }
    }

    /// The message [wait_message](Self::wait_message) received, until [consume](Self::consume).
    pub fn message(&self) -> Option<(u8, &[u8])> {
        let s = self.start;
        self.done
            .then(|| (self.buf[s + 4], &self.buf[s + 5..s + 4 + self.len()]))
    }

    /// Done with the current message, the next wait reads the next one.
    pub fn consume(&mut self) {
        if self.done {
            self.start += 4 + self.len();
            if self.start == self.end {
                self.start = 0;
                self.end = 0;
            }
            self.done = false;
        }
    }

    fn len(&self) -> usize {
        u32::from_le_bytes(self.buf[self.start..self.start + 4].try_into().unwrap()) as usize
    }

    async fn fill(&mut self) -> io::Result<()> {
        // a message must fit from `start` on: move what is left of the last read to the front
        if self.buf.len() - self.start < 4 + self.max {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        // one read per await: dropping the future loses nothing, the bytes are in buf already
        let n = self.r.read(&mut self.buf[self.end..]).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.end += n;
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

    /// Many messages written at once come out one by one, also when one straddles the end of the read buffer.
    #[tokio::test]
    async fn batches_across_the_buffer_end() {
        let (a, b) = tokio::io::duplex(100_000);
        let mut w = MessageWriter::new(a);
        let mut r = MessageReader::new(b, 5000);
        let sizes: Vec<usize> = (0..200).map(|i| (i * 37) % 5000).collect();
        for (i, len) in sizes.iter().enumerate() {
            w.push(i as u8, &vec![i as u8; *len]);
        }
        let writer = tokio::spawn(async move { w.flush().await.unwrap() });
        for (i, len) in sizes.iter().enumerate() {
            let (kind, payload) = r.read_message().await.unwrap();
            assert_eq!(kind, i as u8);
            assert_eq!(payload, &vec![i as u8; *len][..]);
        }
        writer.await.unwrap();
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
