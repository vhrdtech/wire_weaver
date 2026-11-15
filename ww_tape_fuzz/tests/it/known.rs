//! Known failures: inputs on which ww_tape panics, allocates over the cap or breaks a property a target checks, found
//! by these tests and the fuzz targets and not fixed yet. Each names the target, its seed file (under
//! `seeds/<target>/`) and the place of the failure. A case failing at a known place of its target passes; once the fix
//! lands, the seed passes too and the replay test asks for the entry to go.

use ww_tape_fuzz::case::Outcome;

/// One known failure.
pub struct Known {
    /// The fuzz target, `tape-<name>` or `tape-<group>-hostile`.
    pub target: &'static str,
    /// The seed file that shows it, under `seeds/<target>/`.
    pub seed: &'static str,
    /// Where it fails: the panic's `file:line`, or `alloc` for an allocation over the cap.
    pub at: &'static str,
    /// The panic message and its cause in ww_tape.
    pub message: &'static str,
}

/// Every known failure.
pub const KNOWN: &[Known] = &[
    Known {
        target: "tape-index-hostile",
        seed: "known-reader-entries-past-the-piece",
        at: "ww_tape_fuzz/src/targets/index.rs:51",
        message: "assertion failed: p.start + e.offset < data.len() as u64: TapeReader::entries \
                  (ww_tape/src/reader.rs:761) hands out a final index's entries unchecked against the piece",
    },
    Known {
        target: "tape-reader-hostile",
        seed: "known-reader-entries-past-the-piece",
        at: "ww_tape_fuzz/src/targets/index.rs:51",
        message: "assertion failed: p.start + e.offset < data.len() as u64: TapeReader::entries \
                  (ww_tape/src/reader.rs:761) hands out a final index's entries unchecked against the piece",
    },
    Known {
        target: "tape-reader-hostile",
        seed: "known-prefix-and-salvage-walks-differ",
        at: "ww_tape_fuzz/src/targets/reader.rs:72",
        message: "assertion `left == right` failed: in a file whose layout allows gaps, FileBlocks' prefix walk \
                  goes from a bad block to the next resume place, the salvage walk resyncs byte by byte and hands \
                  out the good blocks in between (ww_tape/src/file.rs)",
    },
    Known {
        target: "tape-reader",
        seed: "fix-67e392294-duplicate-after-a-gap",
        at: "ww_tape_fuzz/src/targets/reader.rs:72",
        message: "assertion `left == right` failed: a block written again after a gap is handed out by the \
                  prefix walk and dropped by the salvage walk (ww_tape/src/file.rs)",
    },
    Known {
        target: "tape-times-hostile",
        seed: "known-chunk-count-unbounded",
        at: "ww_tape_fuzz/src/targets/hostile.rs:71",
        message: "a chunk of 124 bytes reads as 2147483647 items: EventTimes::read and ::salvage take any count \
                  from the head in the implicit time mode (ww_tape/src/times/starts.rs), so a reader's loop over \
                  the items is not bounded by the bytes it read",
    },
    Known {
        target: "tape-summary-hostile",
        seed: "known-node-items-overflow",
        at: "ww_tape/src/summary/mod.rs:245",
        message: "attempt to subtract with overflow: SummaryHead::node_items on a head Summary::read took \
                  (ww_tape/src/summary/mod.rs:243..245, item range past the node)",
    },
];

/// Whether `outcome` of a case of `target` fails at a known place of that target.
pub fn is_known(target: &str, outcome: &Outcome) -> bool {
    outcome
        .place()
        .is_some_and(|at| KNOWN.iter().any(|k| k.target == target && k.at == at))
}
