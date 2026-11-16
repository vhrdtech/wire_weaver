//! [PieceChain]: a recording written as a chain of pieces, cut at chunk boundaries, on files or any [SinkFactory].

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{FinishState, PieceHead, PieceId, PieceIdent, ResultId, TAIL_LEN, TrackStart};
use crate::index::IndexError;
use crate::reader::{ByteSource, TapeReader};
use crate::sink::{Files, SinkFactory};
use crate::writer::{
    Finished, NextPiece, Resumed, TapeWriter, WriteError, WriterOptions, block_id,
};
use crate::{BlockKind, ChunkHead, ColumnSpec, HEADER_LEN, SessionTag};

/// When a [PieceChain] cuts to a new piece; checked before each chunk. Zero or None turns a limit off.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct CutPolicy {
    /// Once a piece holds this many bytes.
    pub bytes: u64,
    /// Once a piece holds this many chunk blocks.
    pub chunks: u64,
    /// Once a piece is this old (since it was started or resumed).
    pub interval: Option<Duration>,
}

/// Name of piece `seq` in its chain's directory: the sequence number in 8 digits, so names sort in chain order.
pub fn piece_name(seq: u64) -> String {
    format!("{seq:08}.wwt")
}

/// Writes one recording as pieces `dir/`[piece_name] (`seq`) on the sinks of a [SinkFactory] (files by default),
/// cutting by a [CutPolicy] before a chunk.
pub struct PieceChain<F: SinkFactory = Files> {
    factory: F,
    dir: PathBuf,
    /// None after a failed cut: the chain takes no more blocks
    writer: Option<TapeWriter<F::Sink>>,
    cut: CutPolicy,
    started: Instant,
    seq: u64,
    finished: Vec<Finished>,
}

/// Options of a new [PieceChain].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct ChainOptions {
    /// Each piece's writer.
    pub writer: WriterOptions,
    pub cut: CutPolicy,
}

impl PieceChain<Files> {
    /// Start chain `chain` of `result` in `dir` (which must exist) with its first piece file, see
    /// [Self::create_on].
    pub fn create(
        dir: &Path,
        result: ResultId,
        chain: u64,
        tracks: &[TrackStart],
        options: ChainOptions,
    ) -> Result<Self, WriteError> {
        Self::create_on(Files, dir, result, chain, tracks, options)
    }

    /// Reopen chain `chain` of `result` in `dir` after a crash, see [Self::resume_on].
    pub fn resume(
        dir: &Path,
        result: ResultId,
        chain: u64,
        options: ChainOptions,
    ) -> Result<(Self, Resumed), WriteError> {
        Self::resume_on(Files, dir, result, chain, options)
    }
}

impl<F: SinkFactory> PieceChain<F> {
    /// Start chain `chain` of `result` under `dir` with its first piece, on the sink `factory` makes, id
    /// [PieceId::first_of] `(result, chain)`; `tracks` say where items of tracks carried over from elsewhere continue
    /// (empty for a new recording). Each chain of a result takes its own number, so its pieces and session tag are its own.
    pub fn create_on(
        mut factory: F,
        dir: &Path,
        result: ResultId,
        chain: u64,
        tracks: &[TrackStart],
        options: ChainOptions,
    ) -> Result<Self, WriteError> {
        let sink = factory.create(&dir.join(piece_name(0)))?;
        let mut writer = TapeWriter::create_on(sink, options.writer.sync);
        writer.set_alignment(options.writer.alignment)?;
        writer.set_index_policy(options.writer.index);
        writer.set_describe_repeat(options.writer.describe_repeat);
        writer.begin_piece(&first_head(result, chain), tracks)?;
        Ok(PieceChain {
            factory,
            dir: dir.to_path_buf(),
            writer: Some(writer),
            cut: options.cut,
            started: Instant::now(),
            seq: 0,
            finished: Vec::new(),
        })
    }

    /// Reopen chain `chain` of `result` under `dir` after a crash: the finished pieces are read from their tails
    /// ([Self::finished]), the unfinished last one is resumed ([TapeWriter::resume_on]; when its head's describe copy
    /// was cut off, the piece before's newest describe set is written again). After a crash between a cut's finish
    /// and the next piece's start, the next piece is started as the cut would have. `options` are the ones the chain
    /// was written with. A chain whose last piece ended complete or aborted is refused ([WriteError::Ended]).
    pub fn resume_on(
        mut factory: F,
        dir: &Path,
        result: ResultId,
        chain: u64,
        options: ChainOptions,
    ) -> Result<(Self, Resumed), WriteError> {
        let mut finished: Vec<(Finished, LastPiece)> = Vec::new();
        let mut seq = 0u64;
        while let Some(src) = factory.finished(&dir.join(piece_name(seq)))? {
            finished.push(read_finished(&src, &dir.join(piece_name(seq)))?);
            seq = seq
                .checked_add(1)
                .ok_or(WriteError::Index(IndexError::Invalid(
                    "piece seq past u64::MAX",
                )))?;
        }
        let path = dir.join(piece_name(seq));
        let prev_describe = finished.last().map(|(_, l)| l.describe.clone());
        let (writer, resumed) = match factory.reopen(&path)? {
            Some(sink) => {
                let (mut w, resumed) =
                    TapeWriter::resume_with(sink, options.writer, prev_describe)?;
                if w.bytes() == 0 {
                    // cut off inside its head: the piece starts again (a first piece without carried-over tracks)
                    match finished.last() {
                        None => w.begin_piece(&first_head(result, chain), &[])?,
                        Some((done, last)) => w.begin_next(next_piece(done, last)?)?,
                    }
                }
                (w, resumed)
            }
            None => {
                let Some((done, last)) = finished.last() else {
                    return Err(WriteError::Io {
                        what: "opening the first piece of the chain",
                        error: std::io::ErrorKind::NotFound.into(),
                    });
                };
                if last.state != FinishState::Continued {
                    return Err(WriteError::Ended(last.state));
                }
                let next = next_piece(done, last)?;
                let sink = factory.create(&path)?;
                let w = TapeWriter::start_next(sink, &options.writer, next)?;
                let resumed = Resumed::started(w.bytes());
                (w, resumed)
            }
        };
        Ok((
            PieceChain {
                factory,
                dir: dir.to_path_buf(),
                writer: Some(writer),
                cut: options.cut,
                started: Instant::now(),
                seq,
                finished: finished.into_iter().map(|(f, _)| f).collect(),
            },
            resumed,
        ))
    }

    /// The piece being written; [WriteError::Failed] after a failed cut.
    pub fn writer(&mut self) -> Result<&mut TapeWriter<F::Sink>, WriteError> {
        self.writer.as_mut().ok_or(WriteError::Failed)
    }

    /// The pieces finished so far, those before a resume included.
    pub fn finished(&self) -> &[Finished] {
        &self.finished
    }

    /// Sequence number of the piece being written.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// The factory making the pieces' sinks, for reading them ([crate::MemTapes::get]).
    pub fn factory(&self) -> &F {
        &self.factory
    }

    /// A records or describe block ([TapeWriter::write_block]).
    pub fn write_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.writer()?.write_block(block)
    }

    /// A whole describe set or addition ([TapeWriter::write_describe]).
    pub fn write_describe<B: AsRef<[u8]>>(&mut self, blocks: &[B]) -> Result<(), WriteError> {
        self.writer()?.write_describe(blocks)
    }

    /// A small block placed in the next gap ([TapeWriter::defer_block]).
    pub fn defer_block(&mut self, block: Vec<u8>) -> Result<(), WriteError> {
        self.writer()?.defer_block(block)
    }

    /// Whether the policy says the current piece is full: checked before a chunk, by its bytes, its chunk blocks
    /// ([TapeWriter::chunks], whoever wrote them) and its age. False after a failed cut.
    pub fn cut_due(&self) -> bool {
        let Some(w) = self.writer.as_ref() else {
            return false;
        };
        let c = self.cut;
        (c.bytes > 0 && w.bytes() >= c.bytes)
            || (c.chunks > 0 && w.chunks() >= c.chunks)
            || c.interval.is_some_and(|t| self.started.elapsed() >= t)
    }

    /// A chunk ([TapeWriter::write_chunk]), in a new piece when [Self::cut_due]: a cut happens before a chunk, never
    /// after the last one, so no piece is left empty.
    pub fn write_chunk(
        &mut self,
        head: &ChunkHead,
        columns: &[(ColumnSpec, &[u8])],
    ) -> Result<(), WriteError> {
        if self.cut_due() {
            self.cut_now()?;
        }
        self.writer()?.write_chunk(head, columns)
    }

    /// Cut to a new piece now.
    pub fn cut_now(&mut self) -> Result<(), WriteError> {
        let seq = self
            .seq
            .checked_add(1)
            .ok_or(WriteError::Index(IndexError::Invalid(
                "piece seq past u64::MAX",
            )))?;
        let next_path = self.dir.join(piece_name(seq));
        let current = self.writer.take().ok_or(WriteError::Failed)?;
        let (done, next) = current.cut_with(&mut self.factory, &next_path)?;
        self.writer = Some(next);
        self.finished.push(done);
        self.seq = seq;
        self.started = Instant::now();
        Ok(())
    }

    /// Finish the last piece as complete; every piece of the chain.
    pub fn finish(self) -> Result<Vec<Finished>, WriteError> {
        self.finish_as(FinishState::Complete)
    }

    /// Finish the last piece in `state` (complete or aborted); every piece of the chain.
    pub fn finish_as(mut self, state: FinishState) -> Result<Vec<Finished>, WriteError> {
        let last = self
            .writer
            .take()
            .ok_or(WriteError::Failed)?
            .finish_as(state)?;
        self.finished.push(last);
        Ok(self.finished)
    }
}

/// The head of the first piece of chain `chain` of `result`.
fn first_head(result: ResultId, chain: u64) -> PieceHead {
    PieceHead {
        ident: PieceIdent {
            id: PieceId::first_of(&result, chain),
            result,
            seq: 0,
        },
        prev: None,
        prev_session: SessionTag::NONE,
    }
}

/// The piece after the finished piece `done`, continued, as its cut would have started it.
fn next_piece(done: &Finished, last: &LastPiece) -> Result<NextPiece, WriteError> {
    NextPiece::after(
        &last.head,
        done,
        last.next_tracks.clone(),
        last.describe.clone(),
        last.session,
        last.next_seq,
    )
}

/// What the piece after a finished one starts from.
struct LastPiece {
    state: FinishState,
    head: PieceHead,
    next_tracks: Vec<TrackStart>,
    /// The newest describe set: the backup before the finish
    describe: Vec<Vec<u8>>,
    session: SessionTag,
    next_seq: u64,
}

/// The finished piece in `src`, named `path`, from its tail, finish, head and describe backup.
fn read_finished<B: ByteSource<Error = std::io::Error>>(
    src: &B,
    path: &Path,
) -> Result<(Finished, LastPiece), WriteError> {
    let bad = |what| WriteError::Index(IndexError::Invalid(what));
    let reader = TapeReader::open(src).map_err(WriteError::Read)?;
    let p = match reader.pieces() {
        [p] if p.start == 0 => p,
        _ => return Err(bad("a finished piece holds one piece")),
    };
    let (Some(tail), Some((finish, ranges)), Some((head, starts))) =
        (p.tail, p.finish.as_ref(), p.head.as_ref())
    else {
        return Err(bad(
            "a finished piece of a chain has a head, a finish and a tail",
        ));
    };
    // the tail carries the session and seq of the last counted block of the piece
    let tail_at = p
        .end
        .checked_sub(crate::reader::wide(TAIL_LEN))
        .ok_or(bad("a piece shorter than its tail"))?;
    let mut header = [0u8; HEADER_LEN];
    src.read_at(tail_at, &mut header)
        .map_err(crate::sink::io_err("reading a tail"))?;
    let id = block_id(&header).ok_or(bad("a tail shorter than a header"))?;
    let describe = newest_describe(&reader, finish.describe)?;
    // the tail's seq is the last counted block's, or 0 (a first piece) or the piece before's when it holds none
    let next_seq = if head.prev.is_none() && !holds_counted(&reader)? {
        0
    } else {
        id.seq.wrapping_add(1)
    };
    Ok((
        Finished {
            path: path.to_path_buf(),
            hash: finish.hash,
            len: tail.piece_len,
            ident: finish.ident,
            next: finish.next,
        },
        LastPiece {
            state: finish.state,
            head: *head,
            next_tracks: crate::index::next_starts(starts, ranges),
            describe,
            session: id.session,
            next_seq,
        },
    ))
}

/// The describe backup at `at` in the only piece of `reader`: the whole set starting there; empty when there is none.
fn newest_describe<B: ByteSource<Error = std::io::Error>>(
    reader: &TapeReader<B>,
    at: Option<u64>,
) -> Result<Vec<Vec<u8>>, WriteError> {
    let Some(at) = at else {
        return Ok(Vec::new());
    };
    let entries = reader
        .track_entries(0, u64::from(crate::DESCRIBE_TRACK))
        .map_err(WriteError::Read)?;
    let mut set = Vec::new();
    let mut next = at;
    let mut buf = Vec::new();
    for e in entries
        .iter()
        .filter(|e| e.kind == BlockKind::Describe && e.offset >= at)
    {
        if e.offset != next {
            break;
        }
        let b = reader
            .read_block(0, e, &mut buf)
            .map_err(WriteError::Read)?;
        let last = b.flags & crate::FLAG_SET_LAST != 0;
        next = e.offset.saturating_add(crate::reader::wide(b.len()));
        set.push(b.bytes().to_vec());
        if last {
            return Ok(set);
        }
    }
    Ok(Vec::new())
}

/// Whether the only piece of `reader` holds a block its writer counted: one the index lists that isn't a describe
/// copy.
fn holds_counted<B: ByteSource<Error = std::io::Error>>(
    reader: &TapeReader<B>,
) -> Result<bool, WriteError> {
    let mut buf = Vec::new();
    for e in reader.entries(0).map_err(WriteError::Read)? {
        if e.kind != BlockKind::Describe {
            return Ok(true);
        }
        if !reader
            .read_block(0, &e, &mut buf)
            .map_err(WriteError::Read)?
            .is_copy()
        {
            return Ok(true);
        }
    }
    Ok(false)
}
