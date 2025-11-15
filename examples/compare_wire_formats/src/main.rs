use bincode::Encode;
use serde::{Deserialize, Serialize};
use wire_weaver::prelude::*;

#[derive_shrink_wrap]
struct Request<'i> {
    pub seq: u16,
    pub path_kind: PathKind<'i>,
    pub kind: RequestKind<'i>,

    // Relocate is_some flags here to avoid loosing 7 bits on padding on each Option
    #[flag]
    pub dummy_c: bool,
    #[flag]
    pub dummy_b: bool,
    #[flag]
    pub dummy_a: bool,
    // As an example use the remaining 5 bits of would-be-padding as well
    pub flags: [bool; 5],

    pub dummy_a: Option<u8>,
    pub dummy_b: Option<u8>,
    pub dummy_c: Option<u8>,

    pub strs: RefVec<'i, &'i str>,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
enum PathKind<'i> {
    Absolute { path: RefVec<'i, UNib32> },
    GlobalCompact,
    GlobalFull,
}

#[derive_shrink_wrap]
#[ww_repr(u4)]
enum RequestKind<'i> {
    Call { args: RefVec<'i, u8> },
    Read,
}

#[derive(Serialize, Deserialize, Encode)]
struct RequestSerde {
    pub seq: u16,
    pub path_kind: PathKindSerde,
    pub kind: RequestKindSerde,
    pub flags: [bool; 5],
    pub dummy_a: Option<u8>,
    pub dummy_b: Option<u8>,
    pub dummy_c: Option<u8>,
    pub strs: Vec<String>,
}

#[derive(Serialize, Deserialize, Encode)]
enum PathKindSerde {
    Absolute { path: Vec<u32> },
    GlobalCompact,
}

#[derive(Serialize, Deserialize, Encode)]
enum RequestKindSerde {
    Call { args: Vec<u8> },
    Read,
}

fn main() {
    let req = Request {
        seq: 1234,
        path_kind: PathKind::Absolute {
            path: RefVec::Slice {
                slice: &[UNib32(0), UNib32(1), UNib32(2)],
            },
        },
        kind: RequestKind::Call {
            args: RefVec::new_bytes(&[0xAA, 0xBB, 0xCC]),
        },
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        flags: [false, true, false, true, false],
        strs: RefVec::Slice {
            slice: &["ab", "c"],
        },
    };
    let mut scratch = [0u8; 128];
    let bytes = req.to_ww_bytes(&mut scratch).unwrap();
    println!("WireWeaver: len: {}: {bytes:02X?}", bytes.len());

    let req = RequestSerde {
        seq: 1234,
        path_kind: PathKindSerde::Absolute {
            path: vec![0, 1, 2],
        },
        kind: RequestKindSerde::Call {
            args: vec![0xAA, 0xBB, 0xCC],
        },
        flags: [false, true, false, true, false],
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: vec!["ab".to_string(), "c".to_string()],
    };

    let bytes = postcard::to_slice(&req, &mut scratch).unwrap();
    println!("Postcard: len: {}: {bytes:02X?}", bytes.len());

    let len = bincode::encode_into_slice(&req, &mut scratch, bincode::config::standard()).unwrap();
    let bytes = &scratch[..len];
    println!("Bincode: len: {}: {bytes:02X?}", bytes.len());

    let bytes = rmp_serde::to_vec(&req).unwrap();
    println!("MessagePack: len: {}: {bytes:02X?}", bytes.len());

    let bytes = serde_json::to_string(&req).unwrap();
    println!("JSON: len: {}: {bytes}", bytes.len());
}
