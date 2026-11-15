//! [check_chain]: how the pieces at hand of one recording link up.

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

use alloc::vec::Vec;

use super::{ContentHash, FinishState, PieceId, PieceIdent};

/// What a reader knows of one piece: its head and, when it was finished, its end.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PieceSummary {
    pub ident: PieceIdent,
    /// The previous piece's content hash and id, from the head; None for the first piece.
    pub prev: Option<(ContentHash, PieceId)>,
    /// State, content hash and next id from the finish or tail; None for a piece being written or cut off.
    pub end: Option<(FinishState, ContentHash, Option<PieceId>)>,
}

/// How two pieces next to each other by sequence number relate.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChainLink {
    /// The later head names the earlier piece by hash and id, and the earlier tail names the later id.
    Linked,
    /// Pieces between them are missing.
    Gap { missing: u64 },
    /// Adjacent sequence numbers, but the hashes or ids don't match: not the same chain, or a piece was rewritten.
    Broken,
    /// The earlier piece has no end: it was cut off and the chain went on regardless.
    Unfinished,
}

/// How the chain ends.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChainEnd {
    /// The last piece says the recording is complete.
    Complete,
    /// The last piece says the recording goes on in a piece that isn't here.
    TailCut,
    Aborted,
    /// The last piece has no end: being written, or cut off by a crash.
    Open,
}

/// [check_chain]'s answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainReport {
    /// Indices into the pieces given, by sequence number.
    pub order: Vec<usize>,
    /// The first piece at hand has a previous piece: the chain's start was deleted (retention), not damaged.
    pub head_cut: bool,
    /// `links[i]` is between `order[i]` and `order[i + 1]`.
    pub links: Vec<ChainLink>,
    pub end: ChainEnd,
    /// Two pieces with one sequence number or of another result than the first: left out of `order`.
    pub strays: Vec<usize>,
}

impl ChainReport {
    /// Every piece there, linked, from the first to a complete end.
    pub fn is_whole(&self) -> bool {
        !self.head_cut
            && self.strays.is_empty()
            && self.end == ChainEnd::Complete
            && self.links.iter().all(|l| *l == ChainLink::Linked)
    }
}

/// Order `pieces` of one recording by sequence number and check each link both ways.
pub fn check_chain(pieces: &[PieceSummary]) -> ChainReport {
    let mut idx: Vec<usize> = (0..pieces.len()).collect();
    idx.sort_by_key(|&i| (pieces[i].ident.seq, i));
    let mut order: Vec<usize> = Vec::with_capacity(idx.len());
    let mut strays = Vec::new();
    for i in idx {
        let same_seq = order
            .last()
            .is_some_and(|&l| pieces[l].ident.seq == pieces[i].ident.seq);
        let other_result = order
            .first()
            .is_some_and(|&f| pieces[f].ident.result != pieces[i].ident.result);
        if same_seq || other_result {
            strays.push(i);
        } else {
            order.push(i);
        }
    }
    let links = order
        .windows(2)
        .map(|w| link(&pieces[w[0]], &pieces[w[1]]))
        .collect();
    let head_cut = order.first().is_some_and(|&f| pieces[f].prev.is_some());
    let end = match order.last().and_then(|&l| pieces[l].end) {
        None => ChainEnd::Open,
        Some((FinishState::Complete, ..)) => ChainEnd::Complete,
        Some((FinishState::Continued, ..)) => ChainEnd::TailCut,
        Some((FinishState::Aborted, ..)) => ChainEnd::Aborted,
    };
    ChainReport {
        order,
        head_cut,
        links,
        end,
        strays,
    }
}

fn link(a: &PieceSummary, b: &PieceSummary) -> ChainLink {
    let missing = b.ident.seq - a.ident.seq - 1;
    if missing > 0 {
        return ChainLink::Gap { missing };
    }
    let Some((_, hash, next)) = a.end else {
        return ChainLink::Unfinished;
    };
    if next == Some(b.ident.id) && b.prev == Some((hash, a.ident.id)) {
        ChainLink::Linked
    } else {
        ChainLink::Broken
    }
}
