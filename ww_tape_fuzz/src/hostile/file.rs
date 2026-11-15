//! Whole pieces for the positional reader and the file walk: a piece head, a describe block with a layout, records
//! blocks, an events chunk, a checkpoint and a final index, a finish and a tail, whose offsets, lengths, layout and
//! stretches between blocks take hostile values.

use super::chunk::{build, reseal};
use super::index::{
    TAIL_FIELDS, finish_block, finish_fields, hit_index, index_block, piece_head_block,
    reseal_index, tail_block,
};
use super::{Draw, Field, RUN, describe_block, get, hit, hit_some, put, records_block, seal};
use shrink_wrap::prelude::*;
use ww_tape::index::{IndexEntry, IndexKind};
use ww_tape::piece::{ContentHash, TAIL_LEN};
use ww_tape::times::EncodedTimes;
use ww_tape::{
    Block, BlockId, BlockKind, Describe, GAP_MAX_MAX, GAP_PROBES_MAX, Layout, Producer, RunTag,
    SD_GAP_MAX, SD_RESUME_ALIGN,
};

/// A layout from the input: none, an SD card's, or one of hostile gap rules.
fn layout(d: &mut Draw<'_>) -> Option<Vec<u8>> {
    let layout = match d.u8() % 4 {
        0 => return None,
        1 => Layout::UNALIGNED.on_sd_card(),
        2 => Layout {
            gap_max: exact_units::Bytes::new(4096),
            resume_align: exact_units::Bytes::new(SD_RESUME_ALIGN),
            ..Layout::UNALIGNED
        },
        _ => {
            let caps = &[
                u64::from(GAP_MAX_MAX),
                u64::from(GAP_PROBES_MAX),
                u64::from(SD_GAP_MAX),
                512,
                4096,
            ];
            let mut v = || exact_units::Bytes::new(d.value(32, caps, &[]) as u32);
            Layout {
                alignment: v(),
                gap_max: v(),
                resume_align: v(),
            }
        }
    };
    let mut buf = [0u8; 64];
    Describe::Layout(layout)
        .to_ww_bytes(&mut buf)
        .ok()
        .map(<[u8]>::to_vec)
}

/// A block placed in the file.
struct Placed {
    at: usize,
    len: usize,
    kind: BlockKind,
}

/// A piece drawn from the input whose offsets, lengths, layout or stretches between blocks take hostile values.
pub fn file(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    let mut f: Vec<u8> = vec![];
    let mut placed: Vec<Placed> = vec![];
    let place = |f: &mut Vec<u8>, placed: &mut Vec<Placed>, b: &[u8], kind| {
        placed.push(Placed {
            at: f.len(),
            len: b.len(),
            kind,
        });
        f.extend_from_slice(b);
    };
    let head = d.bool();
    if head {
        let b = piece_head_block(&mut d, BlockId::new(RUN, 0));
        place(&mut f, &mut placed, &b, BlockKind::PieceHead);
    }
    let mut buf = [0u8; 64];
    let producer = Describe::Producer(Producer { name: "hostile" })
        .to_ww_bytes(&mut buf)
        .map(<[u8]>::to_vec)
        .expect("a producer encodes");
    let layout = layout(&mut d);
    let items: Vec<&[u8]> = core::iter::once(producer.as_slice())
        .chain(layout.as_deref())
        .collect();
    let describe_at = f.len() as u64;
    place(
        &mut f,
        &mut placed,
        &describe_block(BlockId::new(RUN, 0), &items),
        BlockKind::Describe,
    );
    let mut seq = 0;
    for _ in 0..d.below(4) {
        seq += 1;
        let item = vec![d.u8(); d.below(40)];
        let b = records_block(BlockId::new(RUN, seq), &[(2, seq as i64, &item)]);
        place(&mut f, &mut placed, &b, BlockKind::Records);
    }
    if d.bool() {
        seq += 1;
        let starts: Vec<i64> = (0..1 + d.below(40) as i64).map(|i| i * 3).collect();
        let t = EncodedTimes::encode(&starts, None, None).expect("valid times");
        let cols: Vec<_> = t.columns().collect();
        let mut b = build(&t.head.chunk_head(1, 1, 0), &cols);
        ww_tape::set_id(&mut b, BlockId::new(RUN, seq)).expect("a good chunk renumbers");
        place(&mut f, &mut placed, &b, BlockKind::Events);
    }
    // the indexes list every block but the structural ones
    let entries: Vec<IndexEntry> = placed
        .iter()
        .filter_map(|p| {
            let b = Block::read(&f[p.at..p.at + p.len]).ok()?;
            IndexEntry::of_block(p.at as u64, &b)
        })
        .collect();
    let id = BlockId::new(RUN, seq);
    let checkpoint_at = f.len() as u64;
    let mut b = index_block(&mut d, id, IndexKind::Checkpoint, &entries);
    put(&mut b, 40, 8, checkpoint_at);
    put(&mut b, 48, 8, u64::MAX);
    reseal_index(&mut b);
    place(&mut f, &mut placed, &b, BlockKind::Index);
    let mut sorted = entries.clone();
    sorted.sort_by_key(|e| (e.track_id, e.offset));
    let final_at = f.len() as u64;
    let mut b = index_block(&mut d, id, IndexKind::Final, &sorted);
    put(&mut b, 40, 8, final_at);
    put(&mut b, 48, 8, u64::MAX);
    reseal_index(&mut b);
    place(&mut f, &mut placed, &b, BlockKind::Index);
    let finish_at = f.len();
    // the content hash is filled in at the end, over the bytes as they are then
    let hash = ContentHash([0; 32]);
    let offsets = [Some(checkpoint_at), Some(final_at), Some(describe_at)];
    let b = finish_block(&mut d, id, offsets, hash);
    let finish_len = b.len();
    place(&mut f, &mut placed, &b, BlockKind::Finish);
    let piece_len = (f.len() + TAIL_LEN) as u64;
    let b = tail_block(id, piece_len, finish_at as u64, finish_len as u32, hash);
    place(&mut f, &mut placed, &b, BlockKind::Tail);

    // hostile values over one block, or a stretch between blocks
    let p = &placed[d.below(placed.len())];
    let (at, len, kind) = (p.at, p.len, p.kind);
    let how = d.u8() % 6;
    if how < 4 {
        hit_block(&mut d, &mut f[at..at + len], kind, piece_len);
    }
    match how {
        4 => {
            // a blank or bad stretch before a block
            let fill = [0x00, 0xff, 0x5a][d.below(3)];
            let n = d.value(13, &[512, 4096, 4097], &[len as u64]) as usize;
            f.splice(at..at, core::iter::repeat_n(fill, n));
        }
        5 => {
            // the file cut short, or longer with stale blocks of another run after it
            if d.bool() {
                f.truncate(d.below(f.len()));
            } else {
                let stale =
                    records_block(BlockId::new(RunTag::writable(0xbad), 9), &[(2, 9, &[1; 8])]);
                f.extend_from_slice(&stale);
            }
        }
        _ => {}
    }
    // the content hash of the bytes before the finish, so verify compares what the reader read; a draw leaves it 0
    if d.u8() % 4 != 3
        && let Some(fin) = f.get(finish_at..finish_at + finish_len)
        && get(fin, 5, 1) == u64::from(BlockKind::Finish.to_u8())
    {
        let hash = blake3::hash(&f[..finish_at]);
        f[finish_at + 40..finish_at + 72].copy_from_slice(hash.as_bytes());
        seal(&mut f[finish_at..finish_at + finish_len]);
        let tail_at = finish_at + finish_len;
        if let Some(tail) = f.get_mut(tail_at..tail_at + TAIL_LEN) {
            tail[56..88].copy_from_slice(hash.as_bytes());
            seal(tail);
        }
    }
    f
}

/// Hostile values over the block `block` of `kind`, its CRCs computed again.
fn hit_block(d: &mut Draw<'_>, block: &mut [u8], kind: BlockKind, piece_len: u64) {
    match kind {
        BlockKind::Index => {
            hit_index(d, block);
            reseal_index(block);
        }
        BlockKind::Finish => {
            let tracks = get(block, 12, 4) as usize;
            let len = [piece_len];
            let fields = finish_fields(d, tracks, &len);
            hit_some(d, block, &fields);
            seal(block);
        }
        BlockKind::Tail => {
            hit_some(d, block, &TAIL_FIELDS);
            seal(block);
        }
        BlockKind::PieceHead => {
            hit_some(d, block, &[Field::new(88, 4, &[1]), Field::new(92, 4, &[])]);
            seal(block);
        }
        BlockKind::Events => {
            super::chunk::hit_head(d, block);
            let stale = d.bool();
            reseal(block, stale);
        }
        _ => {
            hit(d, block, super::LEN_FIELD);
            seal(block);
        }
    }
}
