use prost::Message;
use serde::Serialize;
use std::fmt::Write as _;
use wire_weaver::prelude::*;

#[allow(dead_code)]
// the request_generated::wire_sizes::*Builder types, unused: we build the owned structs directly
mod request_generated;

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

/// Hand-written protobuf types (EX-20): no `.proto` file and no `prost-build`, so `just check`/`just test` need
/// no `protoc`. Field tags are picked by hand, as prost-build would from a schema.
mod proto {
    use prost::{Message, Oneof};

    #[derive(Clone, PartialEq, Message)]
    pub struct Empty {}

    #[derive(Clone, PartialEq, Message)]
    pub struct AbsolutePath {
        #[prost(uint32, repeated, tag = "1")]
        pub path: Vec<u32>,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct CallArgs {
        #[prost(bytes = "vec", tag = "1")]
        pub args: Vec<u8>,
    }

    #[derive(Clone, PartialEq, Oneof)]
    pub enum PathKind {
        #[prost(message, tag = "2")]
        Absolute(AbsolutePath),
        #[prost(message, tag = "3")]
        GlobalCompact(Empty),
        #[prost(message, tag = "4")]
        GlobalFull(Empty),
    }

    #[derive(Clone, PartialEq, Oneof)]
    pub enum RequestKind {
        #[prost(message, tag = "5")]
        Call(CallArgs),
        #[prost(message, tag = "6")]
        Read(Empty),
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct Request {
        #[prost(uint32, tag = "1")]
        pub seq: u32,
        #[prost(oneof = "PathKind", tags = "2,3,4")]
        pub path_kind: Option<PathKind>,
        #[prost(oneof = "RequestKind", tags = "5,6")]
        pub request_kind: Option<RequestKind>,
        #[prost(bool, repeated, tag = "7")]
        pub flags: Vec<bool>,
        #[prost(uint32, optional, tag = "8")]
        pub dummy_a: Option<u32>,
        #[prost(uint32, optional, tag = "9")]
        pub dummy_b: Option<u32>,
        #[prost(uint32, optional, tag = "10")]
        pub dummy_c: Option<u32>,
        #[prost(string, repeated, tag = "11")]
        pub strs: Vec<String>,
    }
}

/// ASN.1 types for rasn (EX-20): the same message, closed (no `...`) and extensible. The extensible version
/// costs one more bit per UPER-encoded value (the extension presence bit) and in exchange lets a later revision
/// append `extension_addition` fields that an old decoder can skip; the closed version can never gain a field
/// without breaking every decoder in the field.
mod asn1 {
    use rasn::prelude::*;

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(choice, automatic_tags)]
    pub enum PathKind {
        Absolute(SequenceOf<u32>),
        GlobalCompact(()),
        GlobalFull(()),
    }

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(choice, automatic_tags)]
    pub enum RequestKind {
        Call(OctetString),
        Read(()),
    }

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(automatic_tags)]
    pub struct Request {
        pub seq: u16,
        pub path_kind: PathKind,
        pub request_kind: RequestKind,
        pub flags: SequenceOf<bool>,
        pub dummy_a: Option<u8>,
        pub dummy_b: Option<u8>,
        pub dummy_c: Option<u8>,
        pub strs: SequenceOf<Utf8String>,
    }

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(automatic_tags)]
    #[non_exhaustive]
    pub struct RequestExtensible {
        pub seq: u16,
        pub path_kind: PathKind,
        pub request_kind: RequestKind,
        pub flags: SequenceOf<bool>,
        pub dummy_a: Option<u8>,
        pub dummy_b: Option<u8>,
        pub dummy_c: Option<u8>,
        pub strs: SequenceOf<Utf8String>,
    }
}

/// Whether old and new ends of this format can read each other's messages: `evolvable` when a schema change (a
/// field appended, in the common case) is routinely backwards and forwards compatible; `closed` when the format
/// has no mechanism for that and a schema change is a breaking wire change.
#[derive(Clone, Copy)]
enum Evolution {
    Evolvable,
    Closed,
}

impl Evolution {
    fn as_str(self) -> &'static str {
        match self {
            Evolution::Evolvable => "evolvable",
            Evolution::Closed => "closed",
        }
    }
}

struct Row {
    format: &'static str,
    bytes: Vec<u8>,
    evolution: Evolution,
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

    let mut rows = Vec::new();

    let bytes = req.to_ww_bytes_owned().unwrap();
    rows.push(Row {
        format: "WireWeaver",
        bytes,
        evolution: Evolution::Evolvable,
    });

    let mut scratch = [0u8; 128];
    let bytes = postcard::to_slice(&req, &mut scratch).unwrap().to_vec();
    rows.push(Row {
        format: "Postcard",
        bytes,
        evolution: Evolution::Closed,
    });

    let bytes = rmp_serde::to_vec(&req).unwrap();
    rows.push(Row {
        format: "MessagePack",
        bytes,
        evolution: Evolution::Closed,
    });

    let bytes = serde_json::to_vec(&req).unwrap();
    rows.push(Row {
        format: "JSON",
        bytes,
        evolution: Evolution::Evolvable,
    });

    let mut bytes = Vec::new();
    ciborium::into_writer(&req, &mut bytes).unwrap();
    rows.push(Row {
        format: "CBOR",
        bytes,
        evolution: Evolution::Closed,
    });

    let bytes =
        <serde_wincode::SerdeCompat<RequestOwned> as wincode::Serialize>::serialize(&req).unwrap();
    rows.push(Row {
        format: "wincode",
        bytes,
        evolution: Evolution::Closed,
    });

    let proto_req = proto::Request {
        seq: 1234,
        path_kind: Some(proto::PathKind::Absolute(proto::AbsolutePath {
            path: vec![0, 1, 2],
        })),
        request_kind: Some(proto::RequestKind::Call(proto::CallArgs {
            args: vec![0xAA, 0xBB, 0xCC],
        })),
        flags: vec![false, true, false, true, false],
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: vec!["ab".to_string(), "c".to_string()],
    };
    let bytes = proto_req.encode_to_vec();
    rows.push(Row {
        format: "Protobuf",
        bytes,
        evolution: Evolution::Evolvable,
    });

    let fb_req = request_generated::wire_sizes::Request {
        seq: 1234,
        path_kind: Some(request_generated::wire_sizes::PathKind::AbsolutePath(
            Box::new(request_generated::wire_sizes::AbsolutePath {
                path: Some(vec![0, 1, 2]),
            }),
        )),
        request_kind: Some(request_generated::wire_sizes::RequestKind::CallArgs(
            Box::new(request_generated::wire_sizes::CallArgs {
                args: Some(vec![0xAA, 0xBB, 0xCC]),
            }),
        )),
        flags: Some(vec![false, true, false, true, false]),
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: Some(vec!["ab".to_string(), "c".to_string()]),
    };
    let mut fb_builder = planus::Builder::new();
    let bytes = fb_builder.finish(&fb_req, None).to_vec();
    rows.push(Row {
        format: "FlatBuffers",
        bytes,
        evolution: Evolution::Evolvable,
    });

    let path_kind_asn1 = asn1::PathKind::Absolute(vec![0, 1, 2]);
    let request_kind_asn1 =
        asn1::RequestKind::Call(rasn::types::OctetString::from(vec![0xAA, 0xBB, 0xCC]));
    let asn1_req = asn1::Request {
        seq: 1234,
        path_kind: path_kind_asn1.clone(),
        request_kind: request_kind_asn1.clone(),
        flags: vec![false, true, false, true, false],
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: vec!["ab".to_string(), "c".to_string()],
    };
    let bytes = rasn::uper::encode(&asn1_req).unwrap();
    rows.push(Row {
        format: "ASN.1 UPER (closed)",
        bytes,
        evolution: Evolution::Closed,
    });

    let asn1_req_ext = asn1::RequestExtensible {
        seq: 1234,
        path_kind: path_kind_asn1,
        request_kind: request_kind_asn1,
        flags: vec![false, true, false, true, false],
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: vec!["ab".to_string(), "c".to_string()],
    };
    let bytes = rasn::uper::encode(&asn1_req_ext).unwrap();
    rows.push(Row {
        format: "ASN.1 UPER (extensible)",
        bytes,
        evolution: Evolution::Evolvable,
    });

    let mut transcript = String::new();
    for row in &rows {
        writeln!(
            transcript,
            "{}: len: {}: {:02X?}",
            row.format,
            row.bytes.len(),
            row.bytes
        )
        .unwrap();
        println!(
            "{}: len: {}: {:02X?}",
            row.format,
            row.bytes.len(),
            row.bytes
        );
    }
    book_output::check("compare_wire_formats/output.txt", &transcript);

    let mut chart = String::from("format,bytes,evolution\n");
    for row in &rows {
        writeln!(
            chart,
            "{},{},{}",
            row.format,
            row.bytes.len(),
            row.evolution.as_str()
        )
        .unwrap();
    }
    book_output::check("compare_wire_formats/sizes.csv", &chart);
}

/// A small, standalone copy of `tests/book_outputs`' stale check: `examples/compare_wire_formats` is excluded
/// from the main workspace (its own `Cargo.lock`, so ciborium/prost/wincode/planus/rasn never reach every other
/// build's lockfile), so it cannot depend on the `book_outputs` crate; this reproduces its contract instead
/// (`docs/generated/<name>`, `WW_BOOK_OUTPUTS=write`) so `just book-outputs`/`just test-all` can drive it the
/// same way via `just`, outside of nextest's hang detector.
mod book_output {
    use std::fs;
    use std::path::{Path, PathBuf};

    const WRITE_ENV: &str = "WW_BOOK_OUTPUTS";
    const GENERATED_DIR: &str = "docs/generated";

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|dir| dir.join("zensical.toml").is_file())
            .expect("compare_wire_formats sits inside the repository of the book")
            .to_path_buf()
    }

    pub fn check(name: &str, text: &str) {
        let write = std::env::var(WRITE_ENV).is_ok_and(|value| value == "write");
        let file = repository_root().join(GENERATED_DIR).join(name);
        if write {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, text).unwrap();
            return;
        }
        let on_disk = fs::read_to_string(&file).unwrap_or_default();
        assert_eq!(
            on_disk,
            text,
            "{} is stale: run `just book-outputs`",
            file.display()
        );
    }
}
