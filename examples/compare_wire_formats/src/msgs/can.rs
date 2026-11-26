//! A batch of CAN frames: a list of small structs, each with a short byte payload.

use crate::asn1::uper;
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, ww_bytes_owned};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct CanFrameOwned {
        pub id: u32,
        pub extended: bool,
        pub dt_us: u16,
        #[serde(with = "serde_bytes")]
        pub data: Vec<u8>,
    }

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct CanBatchOwned {
        pub bus: u8,
        pub frames: Vec<CanFrameOwned>,
    }
}

mod tuned {
    use super::*;

    #[derive_shrink_wrap(final_structure)]
    pub struct CanFrameOwned {
        #[wire(U29)]
        pub id: u32,
        pub extended: bool,
        #[wire(UNib32)]
        pub dt_us: u16,
        pub data: Vec<u8>,
    }

    #[derive_shrink_wrap]
    pub struct CanBatchOwned {
        pub bus: u8,
        pub frames: Vec<CanFrameOwned>,
    }
}

mod proto {
    use prost::Message;

    #[derive(Clone, PartialEq, Message)]
    pub struct CanFrame {
        #[prost(uint32, tag = "1")]
        pub id: u32,
        #[prost(bool, tag = "2")]
        pub extended: bool,
        #[prost(uint32, tag = "3")]
        pub dt_us: u32,
        #[prost(bytes = "vec", tag = "4")]
        pub data: Vec<u8>,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct CanBatch {
        #[prost(uint32, tag = "1")]
        pub bus: u32,
        #[prost(message, repeated, tag = "2")]
        pub frames: Vec<CanFrame>,
    }
}

mod asn1 {
    use crate::asn1::asn1_pair;
    use rasn::prelude::*;

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(automatic_tags)]
    pub struct CanFrame {
        #[rasn(value("0..=536870911"))]
        pub id: u32,
        pub extended: bool,
        pub dt_us: u16,
        #[rasn(size("0..=8"))]
        pub data: OctetString,
    }

    asn1_pair! {
        CanBatch, CanBatchExtensible {
            pub bus: u8,
            pub frames: SequenceOf<CanFrame>,
        }
    }
}

/// Identifier, extended flag, gap to the previous frame in microseconds, payload.
const FRAMES: [(u32, bool, u16, &[u8]); 6] = [
    (
        0x18FF_50E5,
        true,
        0,
        &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
    ),
    (0x123, false, 120, &[0xDE, 0xAD, 0xBE]),
    (0x123, false, 1_000, &[0xDE, 0xAD, 0xBF]),
    (0x7DF, false, 340, &[0x02, 0x01, 0x0C]),
    (0x18DA_F110, true, 85, &[0x10, 0x08]),
    (0x7E8, false, 400, &[0x04, 0x41, 0x0C, 0x1A, 0xF8]),
];

pub fn message() -> Message {
    let bus = 1;
    let value = plain::CanBatchOwned {
        bus,
        frames: FRAMES
            .iter()
            .map(|&(id, extended, dt_us, data)| plain::CanFrameOwned {
                id,
                extended,
                dt_us,
                data: data.to_vec(),
            })
            .collect(),
    };
    let tuned_value = tuned::CanBatchOwned {
        bus,
        frames: FRAMES
            .iter()
            .map(|&(id, extended, dt_us, data)| tuned::CanFrameOwned {
                id,
                extended,
                dt_us,
                data: data.to_vec(),
            })
            .collect(),
    };
    let proto_value = proto::CanBatch {
        bus: bus.into(),
        frames: FRAMES
            .iter()
            .map(|&(id, extended, dt_us, data)| proto::CanFrame {
                id,
                extended,
                dt_us: dt_us.into(),
                data: data.to_vec(),
            })
            .collect(),
    };
    let fb_value = fb::CanBatch {
        bus,
        frames: Some(
            FRAMES
                .iter()
                .map(|&(id, extended, dt_us, data)| fb::CanFrame {
                    id,
                    extended,
                    dt_us,
                    data: Some(data.to_vec()),
                })
                .collect(),
        ),
    };
    let asn1_frames: Vec<asn1::CanFrame> = FRAMES
        .iter()
        .map(|&(id, extended, dt_us, data)| asn1::CanFrame {
            id,
            extended,
            dt_us,
            data: rasn::types::OctetString::from(data.to_vec()),
        })
        .collect();
    let asn1_value = asn1::CanBatch {
        bus,
        frames: asn1_frames.clone(),
    };
    let asn1_ext_value = asn1::CanBatchExtensible {
        bus,
        frames: asn1_frames,
    };
    Message {
        name: "can_batch",
        title: "A batch of six CAN frames",
        rows: Encodings {
            plain: &value,
            postcard_tuned: None,
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
