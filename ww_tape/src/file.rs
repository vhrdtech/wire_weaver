//! Blocks in a file or any byte stream: appended back to back, read one after the other.

use std::io::{self, Read};

use crate::reader::{OwnedBlock, report_len, wide};
use crate::walk::{Head, SessionState, Verdict};
use crate::{
    Block, BlockError, GAP_CHECKS_MAX, HEADER_LEN, MAGIC, Medium, SessionTag, block_len, is_blank,
};

/// Bytes a [FileBlocks] asks its reader for at once, at least: the buffer grows by at most this much ahead of the
/// bytes that came, so a damaged length never allocates what it says.
const READ_STEP: usize = 64 * 1024;

/// A checked block's header and length, kept apart from its bytes: the bytes are lent again without a second check.
#[derive(Copy, Clone, Debug)]
struct Checked {
    /// The header fields, with no bytes
    head: Block<'static>,
    len: usize,
}

impl Checked {
    fn of(b: &Block<'_>) -> Self {
        Checked {
            head: b.detach(),
            len: b.len(),
        }
    }

    /// The block on the bytes it was checked on, at the start of `buf`; None when `buf` is shorter.
    fn on(self, buf: &[u8]) -> Option<Block<'_>> {
        self.head.on(buf.get(..self.len)?)
    }
}

/// An owned block, e.g. one handed over by a producer, checked once when made.
#[derive(Clone, Debug)]
pub struct BlockBuf(OwnedBlock);

impl PartialEq for BlockBuf {
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}

impl Eq for BlockBuf {}

impl BlockBuf {
    /// Check `bytes` as one whole block.
    pub fn new(bytes: Vec<u8>) -> Result<Self, BlockError> {
        OwnedBlock::read(bytes).map(BlockBuf)
    }

    pub fn block(&self) -> Block<'_> {
        self.0.block()
    }

    pub fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0.into_bytes()
    }
}

/// Why a file could not be read further, or what was skipped in it.
#[derive(Debug)]
#[non_exhaustive]
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
    /// A blank or bad stretch of `len` bytes at `offset` that the session's data goes on after, within the layout's
    /// [gap](crate::Layout::gap_max): a writer could not go on there (a failed program, a bad block, a torn unit,
    /// page padding). The walk goes on after it.
    Gap {
        offset: u64,
        len: u64,
    },
    /// A good block at `offset` with the session and seq of the block before it but other bytes: the later copy wins,
    /// it comes next. (A copy with the same bytes is dropped silently.)
    Replaced {
        offset: u64,
        session: SessionTag,
        seq: u64,
    },
    /// [Recovery::Resync]: a good block of session `session` at `offset`, `len` bytes, where no session may start: stale data
    /// (an earlier life of a medium), never data. Skipped by its length.
    Stale {
        offset: u64,
        session: SessionTag,
        len: u64,
    },
}

/// Why a [Recovery::Prefix] walk stopped before the end of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// The bytes there are not a good block (bad magic, length or CRC), and no block of the session follows within the
    /// layout's gap.
    Bad(BlockError),
    /// The file ends inside a block (a writer stopped mid-way), `len` bytes of it are there.
    Truncated { len: u64 },
    /// A data block's seq is not after the one before it in its session: not this file's next block. (Describe, index,
    /// finish, tail and piece head blocks take no part in the order.)
    SeqBackwards { seq: u64, after: u64 },
    /// A good block of session `session` where no session may start ([Medium]): stale data behind the end of data.
    Stale { session: SessionTag },
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
                    Cut::Stale { session } => {
                        write!(f, "a block of another session ({:08x})", session.get())
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
            FileError::Gap { offset, len } => {
                write!(
                    f,
                    "a gap of {len} bytes at byte {offset}, the data goes on after it"
                )
            }
            FileError::Replaced {
                offset,
                session,
                seq,
            } => write!(
                f,
                "block {seq} of session {:08x} again at byte {offset} with other bytes: the later copy is taken",
                session.get()
            ),
            FileError::Stale {
                offset,
                session,
                len,
            } => write!(
                f,
                "a stale block of another session ({:08x}) at byte {offset}, {len} bytes skipped",
                session.get()
            ),
        }
    }
}

impl std::error::Error for FileError {}

/// What a reader does at damage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Recovery {
    /// The longest valid prefix: stop at the first block with a bad magic, length or CRC, a cut one, one of a session
    /// that may not start there, or a seq that goes backwards, and report where ([FileError::Cut]). What a crash
    /// leaves (a cut last block, blank preallocated space) reads as the file up to there. Where the layout allows
    /// gaps ([crate::Layout::gap_max]), a blank or bad stretch the session's data goes on after is a [FileError::Gap],
    /// not the end. The default.
    #[default]
    Prefix,
    /// Salvage: report a bad block and read on at the next block magic one byte after its start ([FileError::Bad]);
    /// no seq check; a block of a session that may not start there is reported ([FileError::Stale]) and skipped. A
    /// chunk block whose head is good (head CRC) but whose payload isn't is handed out instead, with
    /// [Block::is_intact] false and its bad segments marked ([crate::Chunk::bad_segments]), and the walk goes on
    /// after it by its length. Any byte may start a block, also in a file aligned for its chunk payloads: a block is
    /// taken only when its CRCs match.
    Resync,
}

/// Reads blocks one after the other from a byte stream into one reused buffer, see [Recovery] for what happens at
/// damage and [Medium] for where a new session may start. Blank bytes ([is_blank]: 0x00 or 0xFF) where the next block
/// would start, up to the end, are the end (preallocated space, an erased or trimmed medium); on a raw medium so are
/// blank bytes longer than the layout's [gap](crate::Layout::gap_max), and nothing after them is read.
pub struct FileBlocks<R> {
    rd: R,
    recovery: Recovery,
    /// The bytes read and not yet passed over, from the medium offset `offset` on
    buf: Vec<u8>,
    /// The block handed out last, at the start of `buf`
    current: Option<Checked>,
    offset: u64,
    eof: bool,
    /// The session and seq taken so far, the gap rules
    walk: SessionState,
    /// A Prefix walk was cut: nothing more
    done: bool,
    /// The block at the start of `buf` replaces the one taken before with its id (reported already)
    replacing: bool,
}

/// What a step of the walk found.
enum Next {
    /// A block to hand out, at the start of `buf`
    Block(Checked),
    /// A copy of the block before it with the same bytes, passed over
    Dropped,
    End,
}

/// The stream is longer than a `u64` offset goes.
fn too_long() -> FileError {
    FileError::Io(io::Error::from(io::ErrorKind::FileTooLarge))
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
            current: None,
            offset: 0,
            eof: false,
            walk: SessionState::new(Medium::File),
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
    ///
    /// # Panics
    ///
    /// When `layout` is not [valid](crate::Layout::is_valid).
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
        while self.buf.len() < n {
            if self.eof {
                return Ok(false);
            }
            let have = self.buf.len();
            // grown as bytes come, so a damaged length doesn't allocate what it says
            let want = n.min(have.saturating_add(READ_STEP)).max(READ_STEP);
            self.buf.resize(want, 0);
            #[expect(
                clippy::indexing_slicing,
                reason = "buf was just grown to want bytes, and want > have"
            )]
            let dst = &mut self.buf[have..];
            let got = self.rd.read(dst);
            let now = got
                .as_ref()
                .ok()
                .and_then(|got| have.checked_add(*got))
                .filter(|now| *now <= want);
            self.buf.truncate(now.unwrap_or(have));
            match got {
                Ok(0) => self.eof = true,
                Ok(_) if now.is_none() => {
                    return Err(FileError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "the reader said it read more bytes than asked",
                    )));
                }
                Ok(_) => {}
                Err(e) => return Err(FileError::Io(e)),
            }
        }
        Ok(true)
    }

    /// Pass over the first `n` bytes of `buf`.
    fn consume(&mut self, n: usize) -> Result<(), FileError> {
        debug_assert!(n <= self.buf.len(), "consumed bytes are in buf");
        let n = n.min(self.buf.len());
        self.offset = self.offset.checked_add(wide(n)).ok_or_else(too_long)?;
        self.buf.drain(..n);
        Ok(())
    }

    /// Bytes passed over since the medium offset `start`, one this walk passed.
    fn since(&self, start: u64) -> u64 {
        debug_assert!(self.offset >= start, "the walk only goes forward");
        self.offset.saturating_sub(start)
    }

    /// Pass over blank bytes from here: true when they are the end, running to the end of the file or, on a raw
    /// medium, longer than the layout's gap (what follows is no data of this session, and isn't read).
    fn skip_blank(&mut self) -> Result<bool, FileError> {
        let start = self.offset;
        loop {
            let n = self
                .buf
                .iter()
                .position(|b| !is_blank(*b))
                .unwrap_or(self.buf.len());
            self.consume(n)?;
            if self.walk.medium == Medium::Raw && self.since(start) > self.walk.gap_max() {
                self.done = true;
                return Ok(true);
            }
            if !self.buf.is_empty() {
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
        loop {
            match self.step() {
                // a copy with the same bytes as the block before it is dropped
                Ok(Next::Dropped) => {}
                Ok(Next::Block(c)) => {
                    self.current = Some(c);
                    let b = self.current();
                    debug_assert!(b.is_some(), "a block is taken with its bytes in buf");
                    return b.map(Ok);
                }
                Ok(Next::End) => return None,
                Err(e) => return Some(Err(e)),
            }
        }
    }

    /// The block handed out last, at the start of `buf`.
    fn current(&self) -> Option<Block<'_>> {
        self.current?.on(&self.buf)
    }

    /// Find the next block, at the start of `buf`.
    fn step(&mut self) -> Result<Next, FileError> {
        if self.done {
            return Ok(Next::End);
        }
        if let Some(c) = self.current.take() {
            self.consume(c.len)?;
        }
        if !self.fill(1)? {
            return Ok(Next::End);
        }
        // a block starts with the magic, never with a blank byte
        let start = self.offset;
        if self.buf.first().is_some_and(|b| is_blank(*b)) && self.skip_blank()? {
            return Ok(Next::End);
        }
        let blank = self.offset > start;
        let error = match self.block_here()? {
            Here::Good(_) if blank && self.recovery == Recovery::Resync => {
                return Err(FileError::Bad {
                    offset: start,
                    error: BlockError::BadMagic,
                    skipped: self.since(start),
                });
            }
            Here::Good(c) => {
                if blank {
                    // blank bytes, then a block: within the gap rules a gap, else the end of data
                    return self.after_stretch(start, BlockError::BadMagic, Some(c));
                }
                return self.judge(c);
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
                skipped: self.since(start),
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
        let len = match block_len(&self.buf) {
            Ok(len) => len,
            Err(e) => return Ok(Here::Bad(e)),
        };
        if !self.fill(len)? {
            // the rest of the file is in `buf`: a block magic in it means the length was damaged
            let magic_later = self
                .buf
                .get(1..)
                .is_some_and(|rest| rest.windows(MAGIC.len()).any(|w| w == MAGIC));
            if self.recovery == Recovery::Resync && magic_later {
                return Ok(Here::Bad(BlockError::BadLength(report_len(len))));
            }
            return Ok(Here::Truncated);
        }
        Ok(match Block::read(&self.buf) {
            Ok(b) => Here::Good(Checked::of(&b)),
            // a chunk block with a good head: its length holds, the payload is handed out with its bad segments
            // marked, and the walk goes on after it without looking for a magic in the payload
            Err(e) if self.recovery == Recovery::Resync => match Block::salvage(&self.buf) {
                Ok(b) => Here::Good(Checked::of(&b)),
                Err(_) => Here::Bad(e),
            },
            Err(e) => Here::Bad(e),
        })
    }

    /// The good block `c` at the start of `buf`: taken, stale or out of order.
    fn judge(&mut self, c: Checked) -> Result<Next, FileError> {
        let Some(b) = c.on(&self.buf) else {
            return Err(self.truncated());
        };
        let check_seq = self.recovery == Recovery::Prefix;
        let verdict = match self.walk.verdict(&b, check_seq) {
            Verdict::Duplicate { same: false } if self.replacing => Verdict::Take,
            v => v,
        };
        self.replacing = false;
        let (session, seq) = (b.session, b.seq);
        if matches!(verdict, Verdict::Take) {
            self.walk.take(&b);
        }
        match (verdict, self.recovery) {
            (Verdict::Take, _) => Ok(Next::Block(c)),
            (Verdict::Duplicate { same: true }, _) => {
                // dropped: the walk reads on after it
                self.consume(c.len)?;
                Ok(Next::Dropped)
            }
            (Verdict::Duplicate { same: false }, _) => {
                self.replacing = true;
                Err(FileError::Replaced {
                    offset: self.offset,
                    session,
                    seq,
                })
            }
            (Verdict::Stale(session), Recovery::Prefix) => {
                Err(self.cut(self.offset, Cut::Stale { session }))
            }
            (Verdict::Stale(session), Recovery::Resync) => {
                let offset = self.offset;
                self.consume(c.len)?;
                Err(FileError::Stale {
                    offset,
                    session,
                    len: wide(c.len),
                })
            }
            (Verdict::Backwards { seq, after }, _) => {
                Err(self.cut(self.offset, Cut::SeqBackwards { seq, after }))
            }
        }
    }

    /// A blank or bad stretch started at `start` (`buf` is past its blank bytes): the session's data going on within
    /// the layout's gap is a [FileError::Gap], else the stretch is the end of data, cut for `error`. `here` is the
    /// good block right after blank bytes. Probes are read one header at a time, the bytes before each passed over;
    /// at most [GAP_CHECKS_MAX] whose header [may resume](SessionState::may_resume) the walk are checked whole.
    fn after_stretch(
        &mut self,
        start: u64,
        error: BlockError,
        here: Option<Checked>,
    ) -> Result<Next, FileError> {
        let walk = self.walk;
        let in_gap = self
            .offset
            .checked_sub(start)
            .is_some_and(|n| n <= walk.gap_max());
        if in_gap
            && let Some(b) = here.and_then(|c| c.on(&self.buf))
            && walk.resumes(&b)
        {
            return self.gap(start);
        }
        let mut checks = 0;
        for p in walk.probes(start, self.offset) {
            if !self.skip_to(p)? || !self.fill(HEADER_LEN)? {
                break;
            }
            let Some(head) = Head::read(&self.buf) else {
                continue;
            };
            if !walk.may_resume(&head) {
                continue;
            }
            if checks == GAP_CHECKS_MAX {
                break;
            }
            checks = checks.saturating_add(1);
            if !self.fill(head.len)? {
                break;
            }
            if let Some(bytes) = self.buf.get(..head.len)
                && let Ok(b) = Block::read(bytes)
                && walk.resumes(&b)
            {
                return self.gap(start);
            }
        }
        Err(self.cut(start, Cut::Bad(error)))
    }

    /// Pass over the bytes up to medium offset `to`; false when the stream ends before it.
    fn skip_to(&mut self, to: u64) -> Result<bool, FileError> {
        while let Some(ahead) = to.checked_sub(self.offset).filter(|n| *n > 0) {
            if self.buf.is_empty() && !self.fill(1)? {
                return Ok(false);
            }
            let n = usize::try_from(ahead).map_or(self.buf.len(), |n| n.min(self.buf.len()));
            self.consume(n)?;
        }
        Ok(true)
    }

    /// The gap from `start` to here, the walk going on with the block here; an error to report. Here is never
    /// before `start`; if it were, the stretch would be the end of data.
    fn gap(&mut self, start: u64) -> Result<Next, FileError> {
        match self.offset.checked_sub(start) {
            Some(len) => Err(FileError::Gap { offset: start, len }),
            None => Err(self.cut(start, Cut::Bad(BlockError::BadMagic))),
        }
    }

    /// The next good block, None at the end; what was cut, skipped or passed over is handed to `on_error`, a
    /// salvaged chunk block ([Block::is_intact] false) as [FileError::Bad].
    pub fn next_good(&mut self, mut on_error: impl FnMut(FileError)) -> Option<Block<'_>> {
        loop {
            match self.next_block()? {
                Ok(b) if b.is_intact() => break,
                Ok(b) => {
                    let skipped = wide(b.len());
                    on_error(FileError::Bad {
                        offset: self.offset,
                        error: BlockError::BadCrc,
                        skipped,
                    });
                }
                Err(e) => on_error(e),
            }
        }
        self.current()
    }

    fn cut(&mut self, offset: u64, why: Cut) -> FileError {
        self.done = true;
        FileError::Cut { offset, why }
    }

    fn truncated(&mut self) -> FileError {
        let (offset, len) = (self.offset, wide(self.buf.len()));
        if self.recovery == Recovery::Prefix {
            return self.cut(offset, Cut::Truncated { len });
        }
        if let Err(e) = self.consume(self.buf.len()) {
            return e;
        }
        FileError::Truncated { offset, len }
    }

    /// Skip to the next block magic after the start of the current bytes.
    fn resync(&mut self, error: BlockError) -> FileError {
        let offset = self.offset;
        match self.skip_to_magic() {
            Ok(()) => FileError::Bad {
                offset,
                error,
                skipped: self.since(offset),
            },
            Err(e) => e,
        }
    }

    /// Pass over the first byte, then up to the next block magic or the end of the stream.
    fn skip_to_magic(&mut self) -> Result<(), FileError> {
        self.consume(1)?;
        loop {
            if let Some(i) = self.buf.windows(MAGIC.len()).position(|w| w == MAGIC) {
                return self.consume(i);
            }
            // keep the last bytes, a magic may start there
            let keep = MAGIC.len().saturating_sub(1).min(self.buf.len());
            self.consume(self.buf.len().saturating_sub(keep))?;
            let more = self.buf.len().saturating_add(1);
            if !matches!(self.fill(more), Ok(true)) {
                return self.consume(self.buf.len());
            }
        }
    }
}

/// What starts the bytes a [FileBlocks] holds.
enum Here {
    /// A good block (or, in [Recovery::Resync], a chunk with a good head).
    Good(Checked),
    /// The file ends inside the header or the block.
    Truncated,
    Bad(BlockError),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that says it read one byte more than it was asked for.
    struct Overreads;

    impl Read for Overreads {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            Ok(buf.len() + 1)
        }
    }

    #[test]
    fn a_reader_over_its_buffer_is_an_error() {
        let mut blocks = FileBlocks::new(Overreads);
        let e = blocks.next_block().unwrap().unwrap_err();
        assert!(matches!(e, FileError::Io(e) if e.kind() == io::ErrorKind::InvalidData));
    }

    #[test]
    fn an_offset_past_u64_is_an_error() {
        let bytes = [0u8; 16];
        let mut blocks = FileBlocks::new(&bytes[..]).starting_at(u64::MAX - 3);
        let e = blocks.next_block().unwrap().unwrap_err();
        assert!(matches!(e, FileError::Io(e) if e.kind() == io::ErrorKind::FileTooLarge));
    }
}
