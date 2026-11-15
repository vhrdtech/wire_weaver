//! ww_tape's event time codec on any bytes: a chunk block's times read and salvage without panic; times built from
//! the input read back exactly and survive, without panic, a flipped bit and input bytes written over their columns
//! (a good head over a damaged payload, as salvage meets it); every bit-pack kernel decodes any bytes as the scalar
//! code does.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ww_tape::bitpack::{self, GROUP, Kernel};
use ww_tape::chunk::{chunk_len, write_chunk};
use ww_tape::times::{EncodedTimes, Ends, EventTimes};
use ww_tape::{Block, ColumnSpec};

/// Read everything the times give.
fn read_all(t: &EventTimes<'_>) {
    let s = t.starts();
    let mut buf = [0i64; GROUP];
    for i in 0..t.len() {
        let _ = (t.start(i), t.end(i), t.flag(i), t.is_suspect(i));
    }
    for g in 0..s.group_count() {
        assert!(t.decode_group_ends(g, &mut buf) <= GROUP);
        let _ = t.group_flags(g);
        if let Ends::Durations(d) = t.ends() {
            let _ = d.group_bound(g);
        }
    }
    assert_eq!(s.iter().rev().count(), t.len());
    for q in [
        i64::MIN,
        -1,
        0,
        1 << 40,
        i64::MAX,
        s.first_tick(),
        s.last_tick(),
    ] {
        assert!(s.lower_bound(q) <= t.len());
        let _ = s.last_at_or_before(q);
    }
    let _ = t.suspect_items().count();
}

fn walk(block: &[u8]) {
    if let Ok(b) = Block::salvage(block)
        && let Some(c) = b.chunk()
    {
        if let Ok(t) = EventTimes::read(&c) {
            read_all(&t);
        }
        if let Ok(t) = EventTimes::salvage(&c) {
            read_all(&t);
        }
    }
}

fn kernels(data: &[u8]) {
    let Some((&w, src)) = data.split_first() else {
        return;
    };
    let w = u32::from(w) % 65;
    let (base, slope) = (i64::from(w) << 50, (i64::from(w) << 33) - 7);
    let mut want = [0i64; GROUP];
    bitpack::decode_line_with(Kernel::Scalar, src, w, base, slope, &mut want);
    for k in [Kernel::Avx2, Kernel::Avx512, Kernel::Neon] {
        if k.is_available() {
            let mut o = [0i64; GROUP];
            bitpack::decode_line_with(k, src, w, base, slope, &mut o);
            assert_eq!(o, want, "{k:?} width {w}");
        }
    }
}

fuzz_target!(|data: &[u8]| {
    walk(data);
    kernels(data);
    // times from the input: [flip u32][mode u8][first tick u32][patch length u8][patch], then per item a delta u16,
    // a duration u8, a flag u8
    if data.len() < 10 {
        return;
    }
    let flip = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    let mode = data[4];
    let mut t = i64::from_le_bytes([data[5], data[6], data[7], data[8], 0, 0, 0, 0]) << (mode & 31);
    let Some((patch, items)) = data[10..].split_at_checked(usize::from(data[9] % 64)) else {
        return;
    };
    let (mut starts, mut ends, mut flags) = (vec![], vec![], vec![]);
    for c in items.chunks_exact(4) {
        t = t.saturating_add(i64::from(u16::from_le_bytes([c[0], c[1]])) << (mode >> 5));
        starts.push(t);
        ends.push(t.saturating_add(i64::from(c[2])));
        flags.push(u32::from(c[3]) << (c[3] % 25));
    }
    if starts.is_empty() {
        return;
    }
    let enc = EncodedTimes::encode(&starts, Some(&ends), Some(&flags)).expect("valid times");
    let cols: Vec<_> = enc.columns().collect();
    let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
    let mut buf = vec![0u8; chunk_len(&specs, 0)];
    let len =
        write_chunk(&mut buf, 1, &enc.head.chunk_head(1, 1, 0), &cols, 0).expect("a valid chunk");
    let block = Block::read(&buf[..len]).expect("written good");
    let chunk = block.chunk().expect("a chunk");
    let times = EventTimes::read(&chunk).expect("written times read");
    for i in 0..starts.len() {
        assert_eq!(
            (times.start(i), times.end(i), times.flag(i)),
            (starts[i], ends[i], flags[i])
        );
    }
    read_all(&times);
    let p = chunk.payload_offset();
    let at = p + flip % (len - 4 - p);
    let mut flipped = buf.clone();
    flipped[at] ^= 1 << (flip >> 29);
    walk(&flipped[..len]);
    // the patch over the payload, CRCs left as they were: group records that agree with each other but not the column
    let end = (at + patch.len()).min(len - 4);
    buf[at..end].copy_from_slice(&patch[..end - at]);
    walk(&buf[..len]);
});
