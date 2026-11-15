//! ww_tape chunk blocks on any bytes, and on chunks built from the input with one byte flipped: reading and salvaging
//! never panic, a salvaged chunk's head reads back, a byte flipped in the payload of a good chunk costs exactly the
//! segment it is in, and a block given another run and seq stays good.
use ww_tape::chunk::{chunk_len, write_chunk};
use ww_tape::{
    Block, BlockId, BlockKind, ChunkHead, Codec, ColumnKind, ColumnSpec, EndMode, RunTag, set_id,
};

/// Look at everything a block read or salvaged from `data` gives.
fn walk(data: &[u8]) {
    if let Ok(b) = Block::salvage(data) {
        if b.is_intact() {
            assert!(Block::read(data).is_ok());
        }
        if let Some(c) = b.chunk() {
            let _ = c.head();
            for col in c.columns() {
                let _ = c.column_damaged(&col);
            }
            let bad: Vec<usize> = c.bad_segments().collect();
            assert!(bad.iter().all(|s| *s < c.segment_count()));
            if b.is_intact() {
                assert!(bad.is_empty());
            }
        }
    }
    if let Ok(b) = Block::read(data) {
        let mut copy = b.bytes().to_vec();
        let id = BlockId::new(
            RunTag::from_u32(b.run.get().wrapping_add(1)),
            b.seq.wrapping_add(1),
        );
        set_id(&mut copy, id).expect("a good block renumbers");
        let again = Block::read(&copy).expect("still good");
        assert_eq!((again.run, again.seq), (id.run, id.seq));
    }
}

pub fn chunk(data: &[u8]) {
    walk(data);
    // a chunk built from the input: [series?][columns][flip at u32][column bytes...]
    if data.len() < 8 {
        return;
    }
    let series = data[0] & 1 == 1;
    let n = 1 + (data[1] % 4) as usize;
    let flip = u32::from_le_bytes(data[2..6].try_into().unwrap()) as usize;
    let body = &data[6..];
    let per = body.len() / n;
    let mut cols = vec![];
    for i in 0..n {
        let bytes = &body[i * per..(i + 1) * per];
        let kind = if series {
            ColumnKind::Samples
        } else {
            ColumnKind::from_u8(1 + i as u8)
        };
        cols.push((
            ColumnSpec {
                kind,
                codec: Codec::RAW,
                param: i as u16,
                len: bytes.len(),
            },
            bytes,
        ));
    }
    let head = ChunkHead {
        kind: if series {
            BlockKind::Series
        } else {
            BlockKind::Events
        },
        track_id: 7,
        track_number: 1,
        first_item: 0,
        count: 1,
        first_tick: 0,
        last_tick: 0,
        latest_end: 0,
        time_mode: 0,
        end_mode: if series {
            EndMode::Absent
        } else {
            EndMode::Durations
        },
    };
    let specs: Vec<ColumnSpec> = cols.iter().map(|c| c.0).collect();
    let mut buf = vec![0u8; chunk_len(&specs, 0)];
    let len = write_chunk(
        &mut buf,
        BlockId::new(RunTag::writable(3), 3),
        &head,
        &cols,
        0,
    )
    .expect("a valid chunk");
    let block = Block::read(&buf[..len]).expect("written good");
    let chunk = block.chunk().expect("a chunk");
    assert_eq!(chunk.head(), head);
    let at = flip % len;
    let payload = chunk.payload_offset();
    buf[at] ^= 0x01;
    walk(&buf[..len]);
    assert!(Block::read(&buf[..len]).is_err(), "a flipped bit is seen");
    if at >= payload && at < len - 4 {
        let b = Block::salvage(&buf[..len]).expect("a payload flip leaves the head good");
        let bad: Vec<usize> = b.chunk().unwrap().bad_segments().collect();
        assert_eq!(bad, [(at - payload) >> 16]);
    }
}
