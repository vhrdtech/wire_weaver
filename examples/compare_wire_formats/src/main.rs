use serde::Serialize;
use wire_weaver::prelude::*;

#[derive_shrink_wrap(derive(Serialize))]
struct RequestOwned {
    pub seq: u16,
    pub path_kind: PathKindOwned,
    pub kind: RequestKindOwned,

    // Relocate is_some flags here to avoid losing 7 bits on padding on each Option
    #[flag]
    pub dummy_c: bool,
    #[flag]
    pub dummy_b: bool,
    #[flag]
    pub dummy_a: bool,
    // As an example, use the remaining 5 bits of would-be-padding as well
    pub flags: [bool; 5],

    pub dummy_a: Option<u8>,
    pub dummy_b: Option<u8>,
    pub dummy_c: Option<u8>,

    pub strs: Vec<String>,
}

#[derive_shrink_wrap(derive(Serialize), ww_repr = nib)]
enum PathKindOwned {
    Absolute { path: Vec<UNib32> },
    GlobalCompact,
    GlobalFull,
}

#[derive_shrink_wrap(derive(Serialize), ww_repr = nib)]
enum RequestKindOwned {
    Call { args: Vec<u8> },
    Read,
}

fn main() {
    let req = RequestOwned {
        seq: 1234,
        path_kind: PathKindOwned::Absolute {
            path: vec![UNib32(0), UNib32(1), UNib32(2)],
        },
        kind: RequestKindOwned::Call {
            args: vec![0xAA, 0xBB, 0xCC],
        },
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        flags: [false, true, false, true, false],
        strs: vec!["ab".to_string(), "c".to_string()],
    };
    let mut scratch = [0u8; 128];
    let bytes = req.to_ww_bytes_owned().unwrap();
    println!("WireWeaver: len: {}: {bytes:02X?}", bytes.len());

    let bytes = postcard::to_slice(&req, &mut scratch).unwrap();
    println!("Postcard: len: {}: {bytes:02X?}", bytes.len());

    let bytes = rmp_serde::to_vec(&req).unwrap();
    println!("MessagePack: len: {}: {bytes:02X?}", bytes.len());

    let bytes = serde_json::to_string(&req).unwrap();
    println!("JSON: len: {}: {bytes}", bytes.len());
}
