//! Blocks in a file or any byte stream: appended back to back, read one after the other.

use std::io::Read;

use crate::{Block, BlockError, HEADER_LEN, MAGIC, block_len};

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
    /// Bytes at `offset` are not a good block (`error`); `skipped` bytes were passed over to the next block magic.
    Bad {
        offset: u64,
        error: BlockError,
        skipped: u64,
    },
    /// The file ends inside a block (a writer stopped mid-way): `len` bytes from `offset` on are left out.
    Truncated {
        offset: u64,
        len: u64,
    },
}

impl core::fmt::Display for FileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "{e}"),
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

/// Reads blocks one after the other from a byte stream into one reused buffer. A bad block is reported and the
/// reader carries on at the next block magic after it; a cut last block is reported and ends the walk.
pub struct FileBlocks<R> {
    rd: R,
    buf: Vec<u8>,
    /// Bytes of `buf` that hold data, the current block at `buf[..current]`
    filled: usize,
    current: usize,
    /// File offset of `buf[0]`
    offset: u64,
    eof: bool,
}

impl<R: Read> FileBlocks<R> {
    pub fn new(rd: R) -> Self {
        FileBlocks {
            rd,
            buf: Vec::new(),
            filled: 0,
            current: 0,
            offset: 0,
            eof: false,
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
            if self.buf.len() < n.max(64 * 1024) {
                self.buf.resize(n.max(64 * 1024), 0);
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

    /// The next good block, None at the end. Errors are reported one at a time; call again to go on after one.
    pub fn next_block(&mut self) -> Option<Result<Block<'_>, FileError>> {
        if self.current > 0 {
            let n = self.current;
            self.current = 0;
            self.consume(n);
        }
        match self.fill(HEADER_LEN) {
            Ok(true) => {}
            Ok(false) if self.filled == 0 => return None,
            Ok(false) => return Some(Err(self.truncated())),
            Err(e) => return Some(Err(e)),
        }
        let error = match block_len(&self.buf[..self.filled]) {
            Ok(len) => match self.fill(len) {
                Ok(true) => match Block::read(&self.buf[..len]) {
                    Ok(_) => {
                        self.current = len;
                        // re-borrow to tie the block to self
                        return Some(Ok(Block::read(&self.buf[..len]).expect("just read")));
                    }
                    Err(e) => e,
                },
                Ok(false) => return Some(Err(self.truncated())),
                Err(e) => return Some(Err(e)),
            },
            Err(e) => e,
        };
        Some(Err(self.resync(error)))
    }

    /// The next good block, None at the end; a damaged block is handed to `on_error` and skipped.
    pub fn next_good(&mut self, mut on_error: impl FnMut(FileError)) -> Option<Block<'_>> {
        loop {
            match self.next_block()? {
                Ok(_) => break,
                Err(e) => on_error(e),
            }
        }
        Some(Block::read(&self.buf[..self.current]).expect("just read"))
    }

    fn truncated(&mut self) -> FileError {
        let e = FileError::Truncated {
            offset: self.offset,
            len: self.filled as u64,
        };
        let n = self.filled;
        self.consume(n);
        e
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
