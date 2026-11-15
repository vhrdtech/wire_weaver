//! [PieceChain]: a recording written as a chain of piece files, cut at chunk boundaries.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unreachable,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "STD-131: hardening of this module in progress"
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{PieceHead, PieceId, PieceIdent, ResultId, TrackStart};
use crate::writer::{Finished, SyncPolicy, TapeWriter, WriteError};
use crate::{ChunkHead, ColumnSpec, IndexPolicy};

/// When a [PieceChain] cuts to a new piece; checked before each chunk. Zero or None turns a limit off.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct CutPolicy {
    /// Once a piece holds this many bytes.
    pub bytes: u64,
    /// Once a piece holds this many chunk blocks.
    pub chunks: u64,
    /// Once a piece is this old.
    pub interval: Option<Duration>,
}

/// Name of piece `seq` in its chain's directory: the sequence number in 8 digits, so names sort in chain order.
pub fn piece_name(seq: u64) -> String {
    format!("{seq:08}.wwt")
}

/// Writes one recording as piece files `dir/`[piece_name] (`seq`), cutting by a [CutPolicy] after a chunk.
pub struct PieceChain {
    dir: PathBuf,
    /// None after a failed cut: the chain takes no more blocks
    writer: Option<TapeWriter>,
    cut: CutPolicy,
    chunks: u64,
    started: Instant,
    seq: u64,
    finished: Vec<Finished>,
}

/// Options of a new [PieceChain].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct ChainOptions {
    pub sync: SyncPolicy,
    pub index: IndexPolicy,
    pub cut: CutPolicy,
    /// 0 or [crate::ALIGNMENT].
    pub alignment: u32,
}

impl PieceChain {
    /// Start chain `chain` of `result` in `dir` (which must exist) with its first piece, id [PieceId::first_of]
    /// `(result, chain)`; `tracks` say where items of tracks carried over from elsewhere continue (empty for a new
    /// recording). Each chain of a result takes its own number, so its pieces and run tag are its own.
    pub fn create(
        dir: &Path,
        result: ResultId,
        chain: u64,
        tracks: &[TrackStart],
        options: ChainOptions,
    ) -> Result<Self, WriteError> {
        let mut writer = TapeWriter::create(&dir.join(piece_name(0)), options.sync)?;
        writer.set_alignment(options.alignment)?;
        writer.set_index_policy(options.index);
        writer.begin_piece(
            &PieceHead {
                ident: PieceIdent {
                    id: PieceId::first_of(&result, chain),
                    result,
                    seq: 0,
                },
                prev: None,
                prev_run: crate::RunTag::NONE,
            },
            tracks,
        )?;
        Ok(PieceChain {
            dir: dir.to_path_buf(),
            writer: Some(writer),
            cut: options.cut,
            chunks: 0,
            started: Instant::now(),
            seq: 0,
            finished: Vec::new(),
        })
    }

    /// The piece being written; [WriteError::Failed] after a failed cut.
    pub fn writer(&mut self) -> Result<&mut TapeWriter, WriteError> {
        self.writer.as_mut().ok_or(WriteError::Failed)
    }

    /// The pieces finished so far.
    pub fn finished(&self) -> &[Finished] {
        &self.finished
    }

    /// A records or describe block ([TapeWriter::write_block]).
    pub fn write_block(&mut self, block: &[u8]) -> Result<(), WriteError> {
        self.writer()?.write_block(block)
    }

    /// A small block placed in the next gap ([TapeWriter::defer_block]).
    pub fn defer_block(&mut self, block: Vec<u8>) -> Result<(), WriteError> {
        self.writer()?.defer_block(block)
    }

    /// A chunk ([TapeWriter::write_chunk]), in a new piece when the policy says the current one is full: a cut
    /// happens before a chunk, never after the last one, so no piece is left empty.
    pub fn write_chunk(
        &mut self,
        head: &ChunkHead,
        columns: &[(ColumnSpec, &[u8])],
    ) -> Result<(), WriteError> {
        let bytes = self.writer()?.bytes();
        let c = self.cut;
        let due = (c.bytes > 0 && bytes >= c.bytes)
            || (c.chunks > 0 && self.chunks >= c.chunks)
            || c.interval.is_some_and(|t| self.started.elapsed() >= t);
        if due {
            self.cut_now()?;
        }
        self.writer()?.write_chunk(head, columns)?;
        self.chunks += 1;
        Ok(())
    }

    /// Cut to a new piece now.
    pub fn cut_now(&mut self) -> Result<(), WriteError> {
        let next_path = self.dir.join(piece_name(self.seq + 1));
        let current = self.writer.take().ok_or(WriteError::Failed)?;
        let (done, next) = current.cut(&next_path)?;
        self.writer = Some(next);
        self.finished.push(done);
        self.seq += 1;
        self.chunks = 0;
        self.started = Instant::now();
        Ok(())
    }

    /// Finish the last piece as complete; every piece of the chain.
    pub fn finish(mut self) -> Result<Vec<Finished>, WriteError> {
        let last = self.writer.take().ok_or(WriteError::Failed)?.finish()?;
        self.finished.push(last);
        Ok(self.finished)
    }
}
