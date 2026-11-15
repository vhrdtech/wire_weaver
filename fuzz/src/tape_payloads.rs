//! ww_tape's payload codec on any bytes: a chunk block's payloads read and salvage without panic; payloads built
//! from the input read back exactly and survive, without panic, a flipped bit and input bytes written over their
//! columns (a good head over a damaged payload, as salvage meets it), every item not marked suspect reading as
//! written.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ww_tape::chunk::{chunk_len, write_chunk};
use ww_tape::payloads::{EncodedPayloads, Payloads};
use ww_tape::times::EncodedTimes;
use ww_tape::{Block, ColumnSpec};

/// Read everything the payloads give; with `want`, every item not suspect reads as written.
fn read_all(p: &Payloads<'_>, want: Option<&[Vec<u8>]>) {
    for i in 0..=p.len() {
        let got = p.get(i);
        let suspect = p.is_suspect(i);
        if let Some(w) = want.and_then(|w| w.get(i))
            && !suspect
        {
            assert_eq!(got, Some(w.as_slice()), "item {i}");
        }
    }
    assert!(p.get(p.len()).is_none());
    let _ = p.suspect_items().count();
}

fn walk(block: &[u8], want: Option<&[Vec<u8>]>) {
    if let Ok(b) = Block::salvage(block)
        && let Some(c) = b.chunk()
    {
        if b.is_intact()
            && let Ok(Some(p)) = Payloads::read(&c)
        {
            read_all(&p, want);
        }
        if let Ok(Some(p)) = Payloads::salvage(&c) {
            read_all(&p, want);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    walk(data, None);
    // payloads from the input: [flip u32][patch length u8][patch], then per item a length u8 and that many bytes,
    // repeated (length >> 5) times to give lz4 something to find
    if data.len() < 6 {
        return;
    }
    let flip = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let Some((patch, mut rest)) = data[5..].split_at_checked(usize::from(data[4] % 64)) else {
        return;
    };
    let mut items: Vec<Vec<u8>> = Vec::new();
    while let Some((&l, r)) = rest.split_first() {
        let n = usize::from(l & 31).min(r.len());
        let (bytes, r) = r.split_at(n);
        items.push(bytes.repeat(usize::from(l >> 5) + 1));
        rest = r;
    }
    if items.is_empty() {
        return;
    }
    let starts: Vec<i64> = (0..items.len() as i64).collect();
    let times = EncodedTimes::encode(&starts, None, None).expect("valid times");
    let enc = EncodedPayloads::encode(items.iter().map(Vec::as_slice)).expect("valid payloads");
    let cols: Vec<_> = times.columns().chain(enc.columns()).collect();
    let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
    let mut buf = vec![0u8; chunk_len(&specs, 0)];
    let len =
        write_chunk(&mut buf, 1, &times.head.chunk_head(1, 1, 0), &cols, 0).expect("a valid chunk");
    let block = Block::read(&buf[..len]).expect("written good");
    let chunk = block.chunk().expect("a chunk");
    let p = Payloads::read(&chunk).expect("written payloads read").expect("columns");
    for (i, w) in items.iter().enumerate() {
        assert_eq!(p.get(i), Some(w.as_slice()));
        assert!(!p.is_suspect(i));
    }
    let at = chunk.payload_offset() + flip % (len - 4 - chunk.payload_offset());
    let mut flipped = buf.clone();
    flipped[at] ^= 1 << (flip >> 29);
    walk(&flipped[..len], Some(&items));
    // the patch over the payload, CRCs left as they were: records that agree with each other but not the columns.
    // Only a flip is checked against the items: a patch may rewrite a group and its neighbour's link consistently.
    let end = (at + patch.len()).min(len - 4);
    buf[at..end].copy_from_slice(&patch[..end - at]);
    walk(&buf[..len], None);
});
