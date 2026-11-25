//! A firmware update chunk: a 256-byte blob with a small header.

use crate::asn1::{Natural, uper};
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, postcard_bytes, ww_bytes_owned};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct FirmwareChunkOwned {
        pub offset: u32,
        pub total: u32,
        pub crc32: u32,
        pub last: bool,
        #[serde(with = "serde_bytes")]
        pub data: Vec<u8>,
    }
}

mod tuned {
    use super::*;

    #[derive_shrink_wrap]
    pub struct FirmwareChunkOwned {
        #[wire(UVlq32)]
        pub offset: u32,
        #[wire(UVlq32)]
        pub total: u32,
        pub crc32: u32,
        pub last: bool,
        pub data: Vec<u8>,
    }
}

/// Postcard's `fixint` keeps the checksum at four bytes.
mod postcard_tuned {
    use serde::Serialize;

    #[derive(Serialize)]
    pub struct FirmwareChunk {
        pub offset: u32,
        pub total: u32,
        #[serde(with = "postcard::fixint::le")]
        pub crc32: u32,
        pub last: bool,
        #[serde(with = "serde_bytes")]
        pub data: Vec<u8>,
    }
}

mod proto {
    use prost::Message;

    #[derive(Clone, PartialEq, Message)]
    pub struct FirmwareChunk {
        #[prost(uint32, tag = "1")]
        pub offset: u32,
        #[prost(uint32, tag = "2")]
        pub total: u32,
        // fixed32: a CRC is uniformly distributed, a varint would take 5 bytes of it
        #[prost(fixed32, tag = "3")]
        pub crc32: u32,
        #[prost(bool, tag = "4")]
        pub last: bool,
        #[prost(bytes = "vec", tag = "5")]
        pub data: Vec<u8>,
    }
}

mod asn1 {
    use crate::asn1::{Natural, asn1_pair};
    use rasn::prelude::*;

    asn1_pair! {
        FirmwareChunk, FirmwareChunkExtensible {
            pub offset: Natural,
            pub total: Natural,
            pub crc32: u32,
            pub last: bool,
            pub data: OctetString,
        }
    }
}

pub fn message() -> Message {
    let (offset, total, crc32) = (65_536, 262_144, 0x9E37_79B9);
    let data: Vec<u8> = (0..=255u8)
        .map(|i| i.wrapping_mul(37).wrapping_add(11))
        .collect();

    let value = plain::FirmwareChunkOwned {
        offset,
        total,
        crc32,
        last: false,
        data: data.clone(),
    };
    let tuned_value = tuned::FirmwareChunkOwned {
        offset,
        total,
        crc32,
        last: false,
        data: data.clone(),
    };
    let postcard_value = postcard_tuned::FirmwareChunk {
        offset,
        total,
        crc32,
        last: false,
        data: data.clone(),
    };
    let proto_value = proto::FirmwareChunk {
        offset,
        total,
        crc32,
        last: false,
        data: data.clone(),
    };
    let fb_value = fb::FirmwareChunk {
        offset,
        total,
        crc32,
        last: false,
        data: Some(data.clone()),
    };
    let asn1_value = asn1::FirmwareChunk {
        offset: Natural::new(offset),
        total: Natural::new(total),
        crc32,
        last: false,
        data: rasn::types::OctetString::from(data.clone()),
    };
    let asn1_ext_value = asn1::FirmwareChunkExtensible {
        offset: Natural::new(offset),
        total: Natural::new(total),
        crc32,
        last: false,
        data: rasn::types::OctetString::from(data),
    };
    Message {
        name: "firmware_chunk",
        title: "A firmware chunk: a 256-byte blob",
        rows: Encodings {
            plain: &value,
            postcard_tuned: Some(postcard_bytes(&postcard_value)),
            ww_plain: ww_bytes_owned(&value),
            ww_tuned: ww_bytes_owned(&tuned_value),
            protobuf: proto_value.encode_to_vec(),
            flatbuffers: planus::Builder::new().finish(&fb_value, None).to_vec(),
            asn1_closed: uper(&asn1_value),
            asn1_extensible: uper(&asn1_ext_value),
        }
        .into_rows(),
    }
}
