//! Blocks in a file or any byte stream: appended back to back, read one after the other.

use std::io::Read;

use crate::{Block, BlockError, BlockKind, HEADER_LEN, MAGIC, block_len};

/// An owned block, e.g. one handed over by a producer, checked once when made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockBuf(Vec<u8>);

impl BlockBuf {
    /// Check `bytes` as one whole block.
    pub fn new(bytes: Vec<u8>) -> Result<Self, BlockError> {
        let b = Block::read(&bytes)?;
        if b.len() != bytes.len() {
            return Err(BlockError::BadLength(bytes.len() as u32));
        }
        Ok(BlockBuf(bytes))
    }

    pub fn block(&self) -> Block<'_> {
        // checked in new
        Block::read(&self.0).expect("checked")
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// Why a file could not be read further, or what was skipped in it.
#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    /// [Recovery::Prefix]: the file is read up to `offset`, the bytes from there on are left out, for `why`. Comes
    /// once, the walk ends after it.
    Cut {
        offset: u64,
        why: Cut,
    },
    /// [Recovery::Resync]: bytes at `offset` are not a good block (`error`); `skipped` bytes were passed over to the
    /// next block magic.
    Bad {
        offset: u64,
        error: BlockError,
        skipped: u64,
    },
    /// [Recovery::Resync]: the file ends inside a block (a writer stopped mid-way): `len` bytes from `offset` on are
    /// left out.
    Truncated {
        offset: u64,
        len: u64,
    },
}

/// Why a [Recovery::Prefix] walk stopped before the end of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The bytes there are not a good block: bad magic, length or CRC.
    Bad(BlockError),
    /// The file ends inside a block (a writer stopped mid-way), `len` bytes of it are there.
    Truncated { len: u64 },
    /// A data block's seq is not after the one before it: not this file's next block. (A describe block may go
    /// back: it starts the blocks of another producer run appended to the file.)
    SeqBackwards { seq: u64, after: u64 },
}

impl core::fmt::Display for FileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "{e}"),
            FileError::Cut { offset, why } => {
                write!(f, "read up to byte {offset}, the rest is left out: ")?;
                match why {
                    Cut::Bad(e) => write!(f, "not a good block ({e:?})"),
                    Cut::Truncated { len } => {
                        write!(f, "the file ends inside a block ({len} bytes of it)")
                    }
                    Cut::SeqBackwards { seq, after } => {
                        write!(f, "block seq {seq} after {after}")
                    }
                }
            }
            FileError::Bad {
                offset,
                error,
                skipped,
            } => write!(
                f,
                "bad block at byte {offset} ({error:?}), skipped {skipped} bytes"
            ),
            FileError::Truncated { offset, len } => {
                write!(f, "file ends inside a block: {len} bytes at byte {offset}")
            }
        }
    }
}

impl std::error::Error for FileError {}

/// What a reader does at damage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Recovery {
    /// The longest valid prefix: stop at the first block with a bad magic, length or CRC, a cut one, or a seq that
    /// goes backwards, and report where ([FileError::Cut]). What a crash leaves (a cut last block, zeros of
    /// preallocated space) reads as the file up to there. The default.
    #[default]
    Prefix,
    /// Salvage: report a bad block and read on at the next block magic after it ([FileError::Bad]); no seq check.
    Resync,
}

/// Reads blocks one after the other from a byte stream into one reused buffer, see [Recovery] for what happens at
/// damage. Zeros where the next block would start, up to the end, are the end (preallocated space).
pub struct FileBlocks<R> {
    rd: R,
    recovery: Recovery,
    buf: Vec<u8>,
    /// Bytes of `buf` that hold data, the current block at `buf[..current]`
    filled: usize,
    current: usize,
    /// File offset of `buf[0]`
    offset: u64,
    eof: bool,
    /// Seq of the last block given out
    last_seq: Option<u64>,
    /// A Prefix walk was cut: nothing more
    done: bool,
}

impl<R: Read> FileBlocks<R> {
    /// A reader that stops at the first damage ([Recovery::Prefix]).
    pub fn new(rd: R) -> Self {
        Self::with_recovery(rd, Recovery::Prefix)
    }

    pub fn with_recovery(rd: R, recovery: Recovery) -> Self {
        FileBlocks {
            rd,
            recovery,
            buf: Vec::new(),
            filled: 0,
            current: 0,
            offset: 0,
            eof: false,
            last_seq: None,
            done: false,
        }
    }

    /// File offset of the block [Self::next_block] returned last.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Have at least `n` bytes in `buf`, false at the end of the file.
    fn fill(&mut self, n: usize) -> Result<bool, FileError> {
        while self.filled < n {
            if self.eof {
                return Ok(false);
            }
            // grown as bytes come, so a damaged length doesn't allocate what it says
            let want = n.min(self.filled + 64 * 1024).max(64 * 1024);
            if self.buf.len() < want {
                self.buf.resize(want, 0);
            }
            let got = self
                .rd
                .read(&mut self.buf[self.filled..])
                .map_err(FileError::Io)?;
            if got == 0 {
                self.eof = true;
            }
            self.filled += got;
        }
        Ok(true)
    }

    fn consume(&mut self, n: usize) {
        self.buf.copy_within(n..self.filled, 0);
        self.filled -= n;
        self.offset += n as u64;
    }

    /// Whether everything from here to the end of the file is zeros (consumed if so).
    fn zeros_to_end(&mut self) -> Result<bool, FileError> {
        loop {
            if self.buf[..self.filled].iter().any(|b| *b != 0) {
                return Ok(false);
            }
            let n = self.filled;
            self.consume(n);
            if !self.fill(1)? {
                return Ok(true);
            }
        }
    }

    /// The next good block, None at the end. Errors are reported one at a time: after a [FileError::Cut] the walk
    /// is over, after the others call again to go on.
    pub fn next_block(&mut self) -> Option<Result<Block<'_>, FileError>> {
        if self.done {
            return None;
        }
        if self.current > 0 {
            let n = self.current;
            self.current = 0;
            self.consume(n);
        }
        match self.fill(1) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(e) => return Some(Err(e)),
        }
        // a block starts with the magic, never with a zero
        if self.buf[0] == 0 {
            match self.zeros_to_end() {
                Ok(true) => return None,
                Ok(false) => {}
                Err(e) => return Some(Err(e)),
            }
        }
        let error = match self.fill(HEADER_LEN) {
            Ok(true) => match block_len(&self.buf[..self.filled]) {
                Ok(len) => match self.fill(len) {
                    Ok(true) => match Block::read(&self.buf[..len]) {
                        Ok(b) => {
                            let seq = b.seq;
                            // a describe block may start a new producer run (an appended file)
                            if self.recovery == Recovery::Prefix
                                && let Some(after) = self.last_seq
                                && seq <= after
                                && b.kind != BlockKind::Describe
                            {
                                return Some(Err(self.cut(Cut::SeqBackwards { seq, after })));
                            }
                            self.last_seq = Some(seq);
                            self.current = len;
                            // re-borrow to tie the block to self
                            return Some(Ok(Block::read(&self.buf[..len]).expect("just read")));
                        }
                        Err(e) => e,
                    },
                    // the rest of the file is in `buf`: a block magic in it means the length was damaged
                    Ok(false)
                        if self.recovery == Recovery::Resync
                            && self.buf[1..self.filled]
                                .windows(MAGIC.len())
                                .any(|w| w == MAGIC) =>
                    {
                        BlockError::BadLength(len as u32)
                    }
                    Ok(false) => return Some(Err(self.truncated())),
                    Err(e) => return Some(Err(e)),
                },
                Err(e) => e,
            },
            Ok(false) => return Some(Err(self.truncated())),
            Err(e) => return Some(Err(e)),
        };
        Some(Err(match self.recovery {
            Recovery::Prefix => self.cut(Cut::Bad(error)),
            Recovery::Resync => self.resync(error),
        }))
    }

    /// The next good block, None at the end; what was cut or skipped is handed to `on_error`.
    pub fn next_good(&mut self, mut on_error: impl FnMut(FileError)) -> Option<Block<'_>> {
        loop {
            match self.next_block()? {
                Ok(_) => break,
                Err(e) => on_error(e),
            }
        }
        Some(Block::read(&self.buf[..self.current]).expect("just read"))
    }

    fn cut(&mut self, why: Cut) -> FileError {
        self.done = true;
        FileError::Cut {
            offset: self.offset,
            why,
        }
    }

    fn truncated(&mut self) -> FileError {
        let (offset, len) = (self.offset, self.filled as u64);
        if self.recovery == Recovery::Prefix {
            return self.cut(Cut::Truncated { len });
        }
        let n = self.filled;
        self.consume(n);
        FileError::Truncated { offset, len }
    }

    /// Skip to the next block magic after the start of the current bytes.
    fn resync(&mut self, error: BlockError) -> FileError {
        let offset = self.offset;
        let mut skipped = 0u64;
        // skip the first byte, then look for the magic
        self.consume(1);
        skipped += 1;
        loop {
            if let Some(i) = self.buf[..self.filled]
                .windows(MAGIC.len())
                .position(|w| w == MAGIC)
            {
                self.consume(i);
                skipped += i as u64;
                break;
            }
            // keep the last bytes, a magic may start there
            let keep = (MAGIC.len() - 1).min(self.filled);
            let drop = self.filled - keep;
            self.consume(drop);
            skipped += drop as u64;
            match self.fill(self.filled + 1) {
                Ok(true) => {}
                _ => {
                    let n = self.filled;
                    self.consume(n);
                    skipped += n as u64;
                    break;
                }
            }
        }
        FileError::Bad {
            offset,
            error,
            skipped,
        }
    }
}
