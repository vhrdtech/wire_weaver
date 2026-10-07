//! The worked examples of `docs/serdes/use_cases.md`: every size and byte sequence the page quotes is asserted here.

use hex_literal::hex;
use shrink_wrap::BufReader;
use shrink_wrap::prelude::*;

// ---- 1. ring files of a load history (tpm_mesh_dash `src/history.rs`)

const MAX_SERIES: usize = 64;

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct TierHeaderOwned {
    magic: u32,
    size: TailSize<2>,
    period: u32,
    slots: u32,
    names: Vec<String>,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct FineRecordOwned {
    ts: u32,
    size: TailSize<2>,
    values: Vec<f32>,
}

#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy, Default))]
struct Stat {
    min: f32,
    avg: f32,
    max: f32,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct CoarseRecordOwned {
    ts: u32,
    size: TailSize<2>,
    values: Vec<Stat>,
}

const FINE_RECORD: usize = 4 + 2 + MAX_SERIES * 4 + 2;
const COARSE_RECORD: usize = 4 + 2 + MAX_SERIES * 3 * 4 + 2;

#[test]
fn history_header_and_records() {
    let header = TierHeaderOwned {
        magic: u32::from_le_bytes(*b"TMH2"),
        size: TailSize(0),
        period: 1,
        slots: 3600,
        names: vec!["cpu".into(), "mem".into(), "net".into()],
    };
    let mut region = vec![0u8; 4096]; // the header region of the file, zero padded
    let bytes = header.to_ww_bytes_owned().unwrap();
    assert_eq!(
        bytes,
        hex!("54 4D 48 32  80 13  01 00 00 00  10 0E 00 00  63 70 75 6D 65 6D 6E 65 74  33 33")
    );
    region[..bytes.len()].copy_from_slice(&bytes);
    let back = TierHeaderOwned::from_ww_bytes_owned(&region).unwrap();
    assert_eq!(back.names, header.names);
    assert_eq!(back.size, TailSize(19));
    assert_eq!(bytes.len(), 25);

    // a record sized to the series in use
    let fine = |n: usize| FineRecordOwned {
        ts: 1_790_000_000,
        size: TailSize(0),
        values: vec![0.5; n],
    };
    assert_eq!(
        fine(3).to_ww_bytes_owned().unwrap().len(),
        4 + 2 + 3 * 4 + 1
    );
    assert_eq!(
        fine(MAX_SERIES).to_ww_bytes_owned().unwrap().len(),
        FINE_RECORD
    );
    let coarse = |n: usize| CoarseRecordOwned {
        ts: 1_790_000_000,
        size: TailSize(0),
        values: vec![Stat::default(); n],
    };
    assert_eq!(
        coarse(3).to_ww_bytes_owned().unwrap().len(),
        4 + 2 + 3 * 12 + 1
    );
    assert_eq!(
        coarse(MAX_SERIES).to_ww_bytes_owned().unwrap().len(),
        COARSE_RECORD
    );
    assert_eq!(FINE_RECORD, 264);
    assert_eq!(COARSE_RECORD, 776);

    // reading a whole slot: the record's own slot bounds it, the unused tail is ignored
    let mut slot = vec![0xEEu8; FINE_RECORD];
    let rec = fine(3);
    let bytes = rec.to_ww_bytes_owned().unwrap();
    slot[..bytes.len()].copy_from_slice(&bytes);
    let back = FineRecordOwned::from_ww_bytes_owned(&slot).unwrap();
    assert_eq!(back.values, rec.values);
}

// ---- 2. ask / serve frames (tpm_mesh `src/main.rs`, `ask_wire`)

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
pub struct AskRequestOwned {
    pub service: String,
    pub body: Vec<u8>,
}

#[derive_shrink_wrap(owned, ww_repr = u8, derive(Debug, PartialEq))]
pub enum AskReplyOwned {
    Ok(Vec<u8>),
    Err(String),
}

fn frame<T: SerializeShrinkWrapOwned>(msg: &T, cap: usize) -> Result<Vec<u8>, String> {
    let body = msg.to_ww_bytes_owned().map_err(|e| e.to_string())?;
    if body.len() > cap {
        return Err(format!(
            "message of {} bytes is over the {cap} byte cap",
            body.len()
        ));
    }
    let mut wr = BufWriterOwned::with_capacity(body.len() + 5);
    wr.write_uvlq32(body.len() as u32)
        .map_err(|e| e.to_string())?;
    wr.write_raw_slice(&body).map_err(|e| e.to_string())?;
    wr.finish_and_take().map_err(|e| e.to_string())
}

fn unframe<T: DeserializeShrinkWrapOwned>(bytes: &[u8], cap: usize) -> Result<T, String> {
    let mut rd = BufReader::new(bytes);
    let len = rd.read_uvlq32().map_err(|e| e.to_string())? as usize;
    if len > cap {
        return Err(format!("frame of {len} bytes is over the {cap} byte cap"));
    }
    let body = rd.read_raw_slice(len).map_err(|e| e.to_string())?;
    if rd.bytes_left() != 0 {
        return Err(format!("{} bytes after the frame", rd.bytes_left()));
    }
    T::from_ww_bytes_owned(body).map_err(|e| e.to_string())
}

#[test]
fn ask_frames() {
    let req = AskRequestOwned {
        service: "history".into(),
        body: br#"{"range":"24h"}"#.to_vec(),
    };
    let bytes = frame(&req, 16 * 1024).unwrap();
    assert_eq!(bytes.len(), 25);
    assert_eq!(
        bytes,
        hex!("18  68 69 73 74 6F 72 79  7B 22 72 61 6E 67 65 22 3A 22 32 34 68 22 7D  07 97")
    );
    assert_eq!(bytes[0] as usize, bytes.len() - 1);
    assert_eq!(unframe::<AskRequestOwned>(&bytes, 16 * 1024).unwrap(), req);
    assert!(unframe::<AskRequestOwned>(&bytes, 4).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(unframe::<AskRequestOwned>(&trailing, 16 * 1024).is_err());
    assert!(unframe::<AskRequestOwned>(&bytes[..bytes.len() - 1], 16 * 1024).is_err());
    assert!(unframe::<AskRequestOwned>(&hex!("85 80 80 80 00"), 16 * 1024).is_err());
    let big = AskRequestOwned {
        service: "x".into(),
        body: vec![0; 16 * 1024 + 1],
    };
    assert!(frame(&big, 16 * 1024).is_err());

    let reply = AskReplyOwned::Err("no".into());
    let bytes = frame(&reply, 1 << 20).unwrap();
    assert_eq!(unframe::<AskReplyOwned>(&bytes, 1 << 20).unwrap(), reply);
    assert_eq!(bytes, hex!("04 01 6E 6F 02"));
}

// ---- 3. a small gossip message: schema choice

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct SessionOwned {
    name: String,
    busy: bool,
    cpu_percent: u8,
    mem_mib: UVlq32,
}

#[test]
fn gossip_schema() {
    let s = SessionOwned {
        name: "omarchy-m1".into(),
        busy: true,
        cpu_percent: 42,
        mem_mib: UVlq32(512),
    };
    let bytes = s.to_ww_bytes_owned().unwrap();
    assert_eq!(
        bytes,
        hex!("6F 6D 61 72 63 68 79 2D 6D 31  80  2A  84 00  29")
    );
    assert_eq!(SessionOwned::from_ww_bytes_owned(&bytes).unwrap(), s);
    let json = r#"{"name":"omarchy-m1","busy":true,"cpu_percent":42,"mem_mib":512}"#;
    assert_eq!(json.len(), 64);
    assert_eq!(bytes.len(), 15);
}
