//! Records and describe blocks: the clock table, record heads, time deltas, item lengths, the coded body's decoded
//! length and the describe records of a layout.

use super::{Draw, HEADER_FIELDS, HEADER_LEN, LEN_FIELD, RUN, hit, seal};
use shrink_wrap::prelude::*;
use ww_tape::body::DECODED_BODY_MAX;
use ww_tape::lz4::{lz4_encode, lz4_max_len};
use ww_tape::{
    BlockKind, Describe, FLAG_BODY_CODED, GAP_MAX_MAX, GAP_PROBES_MAX, Layout, MAX_BLOCK_CLOCKS,
    MAX_BLOCK_LEN, Producer, SD_GAP_MAX,
};

/// One varint of the body, as written or with a hostile value.
#[derive(Copy, Clone)]
enum Var {
    U(u64),
    I(i64),
    /// An overlong varint: 11 bytes, every one with its continuation bit but the last.
    Overlong,
}

fn put_uvar(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn put_var(out: &mut Vec<u8>, v: Var) {
    match v {
        Var::U(u) => put_uvar(out, u),
        Var::I(i) => put_uvar(out, ((i << 1) ^ (i >> 63)) as u64),
        Var::Overlong => {
            out.extend_from_slice(&[0xff; 10]);
            out.push(0x01);
        }
    }
}

/// Bits of the slot in a record head for `n` clocks.
fn slot_bits(n: u64) -> u32 {
    match n {
        0 | 1 => 0,
        n => u64::BITS - (n - 1).leading_zeros(),
    }
}

/// A describe record's item: a producer, or a layout with hostile gap rules.
fn describe_item(d: &mut Draw<'_>) -> Vec<u8> {
    let mut buf = [0u8; 128];
    let caps: &[u64] = &[
        u64::from(GAP_MAX_MAX),
        u64::from(GAP_PROBES_MAX),
        u64::from(SD_GAP_MAX),
        512,
        4096,
    ];
    let d = if d.bool() {
        Describe::Producer(Producer { name: "hostile" })
    } else {
        let mut v = || exact_units::Bytes::new(d.value(32, caps, &[]) as u32);
        Describe::Layout(Layout {
            alignment: v(),
            gap_max: v(),
            resume_align: v(),
        })
    };
    d.to_ww_bytes(&mut buf)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// A records or describe block whose clock table, record heads and lengths take hostile values.
pub fn records(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    let describe = d.bool();
    // the body as varints and items: clock table, then per record head, delta, length, item
    let clocks = 1 + d.below(3) as u64;
    let mut vars: Vec<Var> = vec![Var::U(clocks)];
    vars.extend((1..=clocks).map(Var::U));
    let bits = slot_bits(clocks);
    let n = d.below(4);
    let mut items: Vec<(usize, Vec<u8>)> = vec![];
    for _ in 0..n {
        let item = if describe {
            describe_item(&mut d)
        } else {
            vec![d.u8(); d.below(24)]
        };
        let track = if describe { 0 } else { 1 + d.below(4) as u64 };
        vars.push(Var::U(track << bits | d.below(clocks as usize) as u64));
        if track != 0 {
            vars.push(Var::I(i64::from(d.u16() as i16)));
        }
        vars.push(Var::U(item.len() as u64));
        items.push((vars.len(), item));
    }
    // hostile values over one to three varints
    let caps: &[u64] = &[
        MAX_BLOCK_CLOCKS as u64,
        DECODED_BODY_MAX as u64,
        MAX_BLOCK_LEN as u64,
        u64::from(u32::MAX),
    ];
    for _ in 0..1 + d.below(3) {
        let at = d.below(vars.len());
        vars[at] = match d.u8() % 4 {
            3 => Var::Overlong,
            2 => Var::I(d.value(64, caps, &[]) as i64),
            _ => Var::U(d.value(64, caps, &[n as u64])),
        };
    }
    let mut body = vec![];
    let mut next_item = items.iter().peekable();
    for (i, v) in vars.iter().enumerate() {
        put_var(&mut body, *v);
        if let Some((_, item)) = next_item.next_if(|(after, _)| *after == i + 1) {
            body.extend_from_slice(item);
        }
    }
    // a coded body: codec byte, decoded length, the coded bytes
    let mut flags = 0u16;
    if d.u8() % 3 == 2 {
        flags |= FLAG_BODY_CODED;
        let codec = [0u8, 1, 2, 0xff][d.below(4)];
        let mut coded = vec![codec];
        let len = d.value(64, &[DECODED_BODY_MAX as u64], &[body.len() as u64]);
        put_uvar(&mut coded, len);
        if codec == 1 {
            let mut out = vec![0u8; lz4_max_len(body.len())];
            let n = lz4_encode(&body, &mut out).expect("output sized by lz4_max_len");
            coded.extend_from_slice(&out[4..n]);
        } else {
            coded.extend_from_slice(&body);
        }
        body = coded;
    }
    let mut b = vec![0u8; HEADER_LEN];
    b[..4].copy_from_slice(&ww_tape::MAGIC);
    b[4] = ww_tape::VERSION;
    b[5] = if describe {
        BlockKind::Describe
    } else {
        BlockKind::Records
    }
    .to_u8();
    b[6..8].copy_from_slice(&flags.to_le_bytes());
    b[12..16].copy_from_slice(&(n as u32).to_le_bytes());
    b[24..28].copy_from_slice(&RUN.get().to_le_bytes());
    b.extend_from_slice(&body);
    b.extend_from_slice(&[0; 4]);
    let len = b.len() as u32;
    b[8..12].copy_from_slice(&len.to_le_bytes());
    match d.u8() % 8 {
        6 => hit(&mut d, &mut b, LEN_FIELD),
        7 => {
            let f = HEADER_FIELDS[d.below(HEADER_FIELDS.len())];
            hit(&mut d, &mut b, f);
        }
        _ => {}
    }
    seal(&mut b);
    b
}
