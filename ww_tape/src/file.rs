//! Blocks in a file or any byte stream: appended back to back, read one after the other.

use std::io::Read;

use crate::walk::{RunState, Verdict};
use crate::{Block, BlockError, HEADER_LEN, MAGIC, Medium, RunTag, block_len, is_blank};

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
    /// A blank or bad stretch of `len` bytes at `offset` that the run's data goes on after, within the layout's
    /// [gap](crate::Layout::gap_max): a writer could not go on there (a failed program, a bad block, a torn unit,
    /// page padding). The walk goes on after it.
    Gap {
        offset: u64,
        len: u64,
    },
    /// A good block at `offset` with the run and seq of the block before it but other bytes: the later copy wins,
    /// it comes next. (A copy with the same bytes is dropped silently.)
    Replaced {
        offset: u64,
        run: RunTag,
        seq: u64,
    },
    /// [Recovery::Resync]: a good block of run `run` at `offset`, `len` bytes, where no run may start: stale data
    /// (an earlier life of a medium), never data. Skipped by its length.
    Stale {
        offset: u64,
        run: RunTag,
        len: u64,
    },
}

/// Why a [Recovery::Prefix] walk stopped before the end of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The bytes there are not a good block (bad magic, length or CRC), and no block of the run follows within the
    /// layout's gap.
    Bad(BlockError),
    /// The file ends inside a block (a writer stopped mid-way), `len` bytes of it are there.
    Truncated { len: u64 },
    /// A data block's seq is not after the one before it in its run: not this file's next block. (Describe, index,
    /// finish, tail and piece head blocks take no part in the order.)
    SeqBackwards { seq: u64, after: u64 },
    /// A good block of run `run` where no run may start ([Medium]): stale data behind the end of data.
    Stale { run: RunTag },
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
                    Cut::Stale { run } => write!(f, "a block of another run ({:08x})", run.get()),
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
            FileError::Gap { offset, len } => {
                write!(
                    f,
                    "a gap of {len} bytes at byte {offset}, the data goes on after it"
                )
            }
            FileError::Replaced { offset, run, seq } => write!(
                f,
                "block {seq} of run {:08x} again at byte {offset} with other bytes: the later copy is taken",
                run.get()
            ),
            FileError::Stale { offset, run, len } => write!(
                f,
                "a stale block of another run ({:08x}) at byte {offset}, {len} bytes skipped",
                run.get()
            ),
        }
    }
}

impl std::error::Error for FileError {}

/// What a reader does at damage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Recovery {
    /// The longest valid prefix: stop at the first block with a bad magic, length or CRC, a cut one, one of a run
    /// that may not start there, or a seq that goes backwards, and report where ([FileError::Cut]). What a crash
    /// leaves (a cut last block, blank preallocated space) reads as the file up to there. Where the layout allows
    /// gaps ([crate::Layout::gap_max]), a blank or bad stretch the run's data goes on after is a [FileError::Gap],
    /// not the end. The default.
    #[default]
    Prefix,
    /// Salvage: report a bad block and read on at the next block magic one byte after its start ([FileError::Bad]);
    /// no seq check; a block of a run that may not start there is reported ([FileError::Stale]) and skipped. A
    /// chunk block whose head is good (head CRC) but whose payload isn't is handed out instead, with
    /// [Block::is_intact] false and its bad segments marked ([crate::Chunk::bad_segments]), and the walk goes on
    /// after it by its length. Any byte may start a block, also in a file aligned for its chunk payloads: a block is
    /// taken only when its CRCs match.
    Resync,
}

/// Reads blocks one after the other from a byte stream into one reused buffer, see [Recovery] for what happens at
/// damage and [Medium] for where a new run may start. Blank bytes ([is_blank]: 0x00 or 0xFF) where the next block
/// would start, up to the end, are the end (preallocated space, an erased or trimmed medium).
pub struct FileBlocks<R> {
    rd: R,
    recovery: Recovery,
    buf: Vec<u8>,
    /// Bytes of `buf` that hold data, the current block at `buf[..current]`
    filled: usize,
    current: usize,
    /// Medium offset of `buf[0]`
    offset: u64,
    eof: bool,
    /// The run and seq taken so far, the gap rules
    walk: RunState,
    /// A Prefix walk was cut: nothing more
    done: bool,
    /// The block at the start of `buf` replaces the one taken before with its id (reported already)
    replacing: bool,
}

impl<R: Read> FileBlocks<R> {
    /// A reader of a file that stops at the first damage ([Recovery::Prefix]).
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
            walk: RunState::new(Medium::File),
            done: false,
            replacing: false,
        }
    }

    /// Read the bytes as kept on `medium`.
    #[must_use]
    pub fn on(mut self, medium: Medium) -> Self {
        self.walk.medium = medium;
        self
    }

    /// The bytes start at `offset` of the medium (a medium layer's hint), so gap probes land on the medium's
    /// [resume](crate::Layout::resume_align) boundaries.
    #[must_use]
    pub fn starting_at(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }

    /// Take `layout`'s gap rules from the start, before a describe block says them (a medium layer that knows the
    /// medium); a [crate::Layout] read later replaces them.
    #[must_use]
    pub fn with_layout(mut self, layout: &crate::Layout) -> Self {
        self.walk.set_layout(layout);
        self
    }

    /// Medium offset of the block [Self::next_block] returned last.
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

    /// Pass over blank bytes from here: true when they run to the end of the file.
    fn skip_blank(&mut self) -> Result<bool, FileError> {
        loop {
            let n = self.buf[..self.filled]
                .iter()
                .position(|b| !is_blank(*b))
                .unwrap_or(self.filled);
            self.consume(n);
            if self.filled > 0 {
                return Ok(false);
            }
            if !self.fill(1)? {
                return Ok(true);
            }
        }
    }

    /// The next good block, None at the end. Errors are reported one at a time: after a [FileError::Cut] the walk
    /// is over, after the others call again to go on.
    pub fn next_block(&mut self) -> Option<Result<Block<'_>, FileError>> {
        let mut step = self.step();
        // a copy with the same bytes as the block before it is dropped
        while matches!(step, Ok(Some(0))) {
            step = self.step();
        }
        match step {
            Ok(Some(len)) => {
                self.current = len;
                // re-borrow to tie the block to self
                Some(Ok(Block::salvage(&self.buf[..len]).expect("just read")))
            }
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }

    /// Find the next block: its length at the start of `buf`, None at the end.
    fn step(&mut self) -> Result<Option<usize>, FileError> {
        if self.done {
            return Ok(None);
        }
        if self.current > 0 {
            let n = self.current;
            self.current = 0;
            self.consume(n);
        }
        if !self.fill(1)? {
            return Ok(None);
        }
        // a block starts with the magic, never with a blank byte
        let start = self.offset;
        if is_blank(self.buf[0]) && self.skip_blank()? {
            return Ok(None);
        }
        let blank = self.offset > start;
        let error = match self.block_here()? {
            Here::Good(_) if blank && self.recovery == Recovery::Resync => {
                return Err(FileError::Bad {
                    offset: start,
                    error: BlockError::BadMagic,
                    skipped: self.offset - start,
                });
            }
            Here::Good(len) => {
                if blank {
                    // blank bytes, then a block: within the gap rules a gap, else the end of data
                    return self.after_stretch(start, BlockError::BadMagic, Some(len));
                }
                return self.judge(len);
            }
            Here::Truncated if !blank => return Err(self.truncated()),
            Here::Truncated => BlockError::BadMagic,
            Here::Bad(e) => e,
        };
        match self.recovery {
            Recovery::Prefix => self.after_stretch(start, error, None),
            Recovery::Resync if blank => Err(FileError::Bad {
                offset: start,
                error,
                skipped: self.offset - start,
            }),
            Recovery::Resync => Err(self.resync(error)),
        }
    }

    /// What starts `buf`: a good block (a damaged chunk with a good head too, in [Recovery::Resync]), a cut one, or
    /// bad bytes.
    fn block_here(&mut self) -> Result<Here, FileError> {
        if !self.fill(HEADER_LEN)? {
            return Ok(Here::Truncated);
        }
        let len = match block_len(&self.buf[..self.filled]) {
            Ok(len) => len,
            Err(e) => return Ok(Here::Bad(e)),
        };
        if !self.fill(len)? {
            // the rest of the file is in `buf`: a block magic in it means the length was damaged
            let magic_later = self.buf[1..self.filled]
                .windows(MAGIC.len())
                .any(|w| w == MAGIC);
            if self.recovery == Recovery::Resync && magic_later {
                return Ok(Here::Bad(BlockError::BadLength(len as u32)));
            }
            return Ok(Here::Truncated);
        }
        Ok(match Block::read(&self.buf[..len]) {
            Ok(_) => Here::Good(len),
            // a chunk block with a good head: its length holds, the payload is handed out with its bad segments
            // marked, and the walk goes on after it without looking for a magic in the payload
            Err(_)
                if self.recovery == Recovery::Resync
                    && Block::salvage(&self.buf[..len]).is_ok() =>
            {
                Here::Good(len)
            }
            Err(e) => Here::Bad(e),
        })
    }

    /// The good block of `len` bytes at the start of `buf`: taken, stale or out of order.
    fn judge(&mut self, len: usize) -> Result<Option<usize>, FileError> {
        let b = Block::salvage(&self.buf[..len]).expect("checked");
        let check_seq = self.recovery == Recovery::Prefix;
        let verdict = match self.walk.verdict(&b, check_seq) {
            Verdict::Duplicate { same: false } if self.replacing => Verdict::Take,
            v => v,
        };
        self.replacing = false;
        match (verdict, self.recovery) {
            (Verdict::Take, _) => {
                self.walk.take(&b);
                Ok(Some(len))
            }
            (Verdict::Duplicate { same: true }, _) => {
                // dropped: the next step reads on after it
                self.current = len;
                Ok(Some(0))
            }
            (Verdict::Duplicate { same: false }, _) => {
                self.replacing = true;
                Err(FileError::Replaced {
                    offset: self.offset,
                    run: b.run,
                    seq: b.seq,
                })
            }
            (Verdict::Stale(run), Recovery::Prefix) => {
                Err(self.cut(self.offset, Cut::Stale { run }))
            }
            (Verdict::Stale(run), Recovery::Resync) => {
                let offset = self.offset;
                self.consume(len);
                Err(FileError::Stale {
                    offset,
                    run,
                    len: len as u64,
                })
            }
            (Verdict::Backwards { seq, after }, _) => {
                Err(self.cut(self.offset, Cut::SeqBackwards { seq, after }))
            }
        }
    }

    /// A blank or bad stretch started at `start` (`buf` is past its blank bytes): the run's data going on within
    /// the layout's gap is a [FileError::Gap], else the stretch is the end of data, cut for `error`. `here` is the
    /// good block right after blank bytes.
    fn after_stretch(
        &mut self,
        start: u64,
        error: BlockError,
        here: Option<usize>,
    ) -> Result<Option<usize>, FileError> {
        let in_gap = self.offset - start <= self.walk.gap_max();
        if in_gap && let Some(len) = here {
            let b = Block::salvage(&self.buf[..len]).expect("checked");
            if self.walk.resumes(&b) {
                return Err(self.gap(start));
            }
        }
        let probes: Vec<u64> = self.walk.probes(start, self.offset).collect();
        for p in probes {
            let r = (p - self.offset) as usize;
            if !self.fill(r + HEADER_LEN)? {
                break;
            }
            let Ok(len) = block_len(&self.buf[r..self.filled]) else {
                continue;
            };
            if !self.fill(r + len)? {
                continue;
            }
            if let Ok(b) = Block::read(&self.buf[r..r + len])
                && self.walk.resumes(&b)
            {
                self.consume(r);
                return Err(self.gap(start));
            }
        }
        Err(self.cut(start, Cut::Bad(error)))
    }

    fn gap(&self, start: u64) -> FileError {
        FileError::Gap {
            offset: start,
            len: self.offset - start,
        }
    }

    /// The next good block, None at the end; what was cut, skipped or passed over is handed to `on_error`, a
    /// salvaged chunk block ([Block::is_intact] false) as [FileError::Bad].
    pub fn next_good(&mut self, mut on_error: impl FnMut(FileError)) -> Option<Block<'_>> {
        loop {
            match self.next_block()? {
                Ok(b) if b.is_intact() => break,
                Ok(b) => {
                    let skipped = b.len() as u64;
                    on_error(FileError::Bad {
                        offset: self.offset,
                        error: BlockError::BadCrc,
                        skipped,
                    });
                }
                Err(e) => on_error(e),
            }
        }
        Some(Block::read(&self.buf[..self.current]).expect("just read"))
    }

    fn cut(&mut self, offset: u64, why: Cut) -> FileError {
        self.done = true;
        FileError::Cut { offset, why }
    }

    fn truncated(&mut self) -> FileError {
        let (offset, len) = (self.offset, self.filled as u64);
        if self.recovery == Recovery::Prefix {
            return self.cut(offset, Cut::Truncated { len });
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

/// What starts the bytes a [FileBlocks] holds.
enum Here {
    /// A good block (or, in [Recovery::Resync], a chunk with a good head) of this length.
    Good(usize),
    /// The file ends inside the header or the block.
    Truncated,
    Bad(BlockError),
}
