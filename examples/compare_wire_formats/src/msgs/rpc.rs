//! A request of the WireWeaver client-server protocol: an enum-heavy header, a few small numbers, optional bytes.

use crate::asn1::{Natural, uper};
use crate::fbs_request::wire_sizes as fb;
use crate::rows::{Encodings, Message};
use prost::Message as _;
use wire_weaver::prelude::*;

/// The message as a plain Rust type, no layout directives: what every other format reads too.
mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct RequestOwned {
        pub seq: u16,
        pub path_kind: PathKindOwned,
        pub kind: RequestKindOwned,
        pub flags: [bool; 5],
        pub dummy_a: Option<u8>,
        pub dummy_b: Option<u8>,
        pub dummy_c: Option<u8>,
        pub strs: Vec<String>,
    }

    #[derive_shrink_wrap(derive(Serialize), ww_repr = u8)]
    pub enum PathKindOwned {
        Absolute { path: Vec<u32> },
        GlobalCompact,
        GlobalFull,
    }

    #[derive_shrink_wrap(derive(Serialize), ww_repr = u8)]
    pub enum RequestKindOwned {
        Call {
            #[serde(with = "serde_bytes")]
            args: Vec<u8>,
        },
        Read,
    }
}

/// The same message with the layout directives of `shrink_wrap`.
mod tuned {
    use super::*;

    #[derive_shrink_wrap]
    pub struct RequestOwned {
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

    #[derive_shrink_wrap(ww_repr = nib)]
    pub enum PathKindOwned {
        Absolute { path: Vec<UNib32> },
        GlobalCompact,
        GlobalFull,
    }

    #[derive_shrink_wrap(ww_repr = nib)]
    pub enum RequestKindOwned {
        Call { args: Vec<u8> },
        Read,
    }
}

/// Hand-written protobuf types: no `.proto` file and no `prost-build`, so no `protoc` is needed. Field tags are
/// picked by hand, as `prost-build` would from a schema; repeated scalars are packed, the proto3 default.
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

mod asn1 {
    use crate::asn1::{Natural, asn1_pair};
    use rasn::prelude::*;

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(choice, automatic_tags)]
    pub enum PathKind {
        Absolute(SequenceOf<Natural>),
        GlobalCompact(()),
        GlobalFull(()),
    }

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(choice, automatic_tags)]
    pub enum RequestKind {
        Call(OctetString),
        Read(()),
    }

    asn1_pair! {
        Request, RequestExtensible {
            pub seq: u16,
            pub path_kind: PathKind,
            pub request_kind: RequestKind,
            #[rasn(size("5"))]
            pub flags: SequenceOf<bool>,
            pub dummy_a: Option<u8>,
            pub dummy_b: Option<u8>,
            pub dummy_c: Option<u8>,
            pub strs: SequenceOf<Utf8String>,
        }
    }
}

pub fn message() -> Message {
    let strs = vec!["ab".to_string(), "c".to_string()];
    let flags = [false, true, false, true, false];
    let args = vec![0xAA, 0xBB, 0xCC];

    let value = plain::RequestOwned {
        seq: 1234,
        path_kind: plain::PathKindOwned::Absolute {
            path: vec![0, 1, 2],
        },
        kind: plain::RequestKindOwned::Call { args: args.clone() },
        flags,
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: strs.clone(),
    };
    let tuned_value = tuned::RequestOwned {
        seq: 1234,
        path_kind: tuned::PathKindOwned::Absolute {
            path: vec![UNib32(0), UNib32(1), UNib32(2)],
        },
        kind: tuned::RequestKindOwned::Call { args: args.clone() },
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        flags,
        strs: strs.clone(),
    };
    let proto_value = proto::Request {
        seq: 1234,
        path_kind: Some(proto::PathKind::Absolute(proto::AbsolutePath {
            path: vec![0, 1, 2],
        })),
        request_kind: Some(proto::RequestKind::Call(proto::CallArgs {
            args: args.clone(),
        })),
        flags: flags.to_vec(),
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: strs.clone(),
    };
    let fb_value = fb::Request {
        seq: 1234,
        path_kind: Some(fb::PathKind::AbsolutePath(Box::new(fb::AbsolutePath {
            path: Some(vec![0, 1, 2]),
        }))),
        request_kind: Some(fb::RequestKind::CallArgs(Box::new(fb::CallArgs {
            args: Some(args.clone()),
        }))),
        flags: Some(flags.to_vec()),
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: Some(strs.clone()),
    };
    let asn1_path = asn1::PathKind::Absolute([0u32, 1, 2].map(Natural::new).to_vec());
    let asn1_kind = asn1::RequestKind::Call(rasn::types::OctetString::from(args));
    let asn1_value = asn1::Request {
        seq: 1234,
        path_kind: asn1_path.clone(),
        request_kind: asn1_kind.clone(),
        flags: flags.to_vec(),
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: strs.clone(),
    };
    let asn1_ext_value = asn1::RequestExtensible {
        seq: 1234,
        path_kind: asn1_path,
        request_kind: asn1_kind,
        flags: flags.to_vec(),
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs,
    };

    let encodings = Encodings {
        plain: &value,
        postcard_tuned: None,
        ww_plain: value
            .to_ww_bytes_owned()
            .expect("shrink_wrap encodes the example"),
        ww_tuned: tuned_value
            .to_ww_bytes_owned()
            .expect("shrink_wrap encodes the example"),
        protobuf: proto_value.encode_to_vec(),
        flatbuffers: planus::Builder::new().finish(&fb_value, None).to_vec(),
        asn1_closed: uper(&asn1_value),
        asn1_extensible: uper(&asn1_ext_value),
    };
    Message {
        name: "rpc_request",
        title: "An RPC request: enum-heavy header, small numbers, optional fields",
        rows: encodings.into_rows(),
    }
}
