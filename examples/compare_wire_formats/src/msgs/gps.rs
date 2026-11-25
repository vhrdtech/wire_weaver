//! A GPS fix: signed integers of different spread, a bounded count and a four-way enum.

use crate::asn1::{Natural, uper};
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, postcard_bytes, ww_bytes};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(borrowed, derive(Serialize), ww_repr = u8)]
    pub enum FixKind {
        NoFix,
        Fix2d,
        Fix3d,
        Dgps,
    }

    #[derive_shrink_wrap(borrowed, derive(Serialize))]
    pub struct GpsFix {
        pub t_ms: u64,
        pub lat_e7: i32,
        pub lon_e7: i32,
        pub alt_mm: i32,
        pub speed_mm_s: u32,
        pub hdop_centi: u16,
        pub sats: u8,
        pub fix: FixKind,
    }
}

mod tuned {
    use super::*;

    #[derive_shrink_wrap(borrowed, ww_repr = u2, sized)]
    pub enum FixKind {
        NoFix,
        Fix2d,
        Fix3d,
        Dgps,
    }

    #[derive_shrink_wrap(borrowed)]
    pub struct GpsFix {
        #[wire(UVlq64)]
        pub t_ms: u64,
        pub lat_e7: i32,
        pub lon_e7: i32,
        #[wire(IVlq32)]
        pub alt_mm: i32,
        #[wire(UVlq32)]
        pub speed_mm_s: u32,
        #[wire(UNib32)]
        pub hdop_centi: u16,
        #[wire(U6)]
        pub sats: u8,
        pub fix: FixKind,
    }
}

/// Postcard's `fixint` keeps latitude and longitude at four bytes each.
mod postcard_tuned {
    use serde::Serialize;

    #[derive(Serialize)]
    pub struct GpsFix {
        pub t_ms: u64,
        #[serde(with = "postcard::fixint::le")]
        pub lat_e7: i32,
        #[serde(with = "postcard::fixint::le")]
        pub lon_e7: i32,
        pub alt_mm: i32,
        pub speed_mm_s: u32,
        pub hdop_centi: u16,
        pub sats: u8,
        pub fix: super::plain::FixKind,
    }
}

mod proto {
    use prost::{Enumeration, Message};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
    #[repr(i32)]
    pub enum FixKind {
        NoFix = 0,
        Fix2d = 1,
        Fix3d = 2,
        Dgps = 3,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct GpsFix {
        #[prost(uint64, tag = "1")]
        pub t_ms: u64,
        // sfixed32: degrees times 1e7 use most of the 32 bits, a varint can take 5 bytes
        #[prost(sfixed32, tag = "2")]
        pub lat_e7: i32,
        #[prost(sfixed32, tag = "3")]
        pub lon_e7: i32,
        #[prost(sint32, tag = "4")]
        pub alt_mm: i32,
        #[prost(uint32, tag = "5")]
        pub speed_mm_s: u32,
        #[prost(uint32, tag = "6")]
        pub hdop_centi: u32,
        #[prost(uint32, tag = "7")]
        pub sats: u32,
        #[prost(enumeration = "FixKind", tag = "8")]
        pub fix: i32,
    }
}

mod asn1 {
    use crate::asn1::{Natural, asn1_pair};
    use rasn::prelude::*;

    #[derive(AsnType, Encode, Decode, Clone, Copy, PartialEq, Debug)]
    #[rasn(enumerated)]
    pub enum FixKind {
        NoFix,
        Fix2d,
        Fix3d,
        Dgps,
    }

    asn1_pair! {
        GpsFix, GpsFixExtensible {
            pub t_ms: Natural,
            pub lat_e7: i32,
            pub lon_e7: i32,
            pub alt_mm: Integer,
            pub speed_mm_s: Natural,
            pub hdop_centi: u16,
            #[rasn(value("0..=63"))]
            pub sats: u8,
            pub fix: FixKind,
        }
    }
}

pub fn message() -> Message {
    let t_ms = 1_764_000_123_456;
    let (lat_e7, lon_e7, alt_mm) = (525_200_066, 134_049_540, 34_500);
    let (speed_mm_s, hdop_centi, sats) = (1_388, 95, 11);

    let value = plain::GpsFix {
        t_ms,
        lat_e7,
        lon_e7,
        alt_mm,
        speed_mm_s,
        hdop_centi,
        sats,
        fix: plain::FixKind::Fix3d,
    };
    let tuned_value = tuned::GpsFix {
        t_ms,
        lat_e7,
        lon_e7,
        alt_mm,
        speed_mm_s,
        hdop_centi,
        sats,
        fix: tuned::FixKind::Fix3d,
    };
    let postcard_value = postcard_tuned::GpsFix {
        t_ms,
        lat_e7,
        lon_e7,
        alt_mm,
        speed_mm_s,
        hdop_centi,
        sats,
        fix: plain::FixKind::Fix3d,
    };
    let proto_value = proto::GpsFix {
        t_ms,
        lat_e7,
        lon_e7,
        alt_mm,
        speed_mm_s,
        hdop_centi: hdop_centi.into(),
        sats: sats.into(),
        fix: proto::FixKind::Fix3d as i32,
    };
    let fb_value = fb::GpsFix {
        t_ms,
        lat_e7,
        lon_e7,
        alt_mm,
        speed_mm_s,
        hdop_centi,
        sats,
        fix: fb::FixKind::Fix3d,
    };
    let asn1_value = asn1::GpsFix {
        t_ms: Natural::new(t_ms),
        lat_e7,
        lon_e7,
        alt_mm: alt_mm.into(),
        speed_mm_s: Natural::new(speed_mm_s),
        hdop_centi,
        sats,
        fix: asn1::FixKind::Fix3d,
    };
    let asn1_ext_value = asn1::GpsFixExtensible {
        t_ms: Natural::new(t_ms),
        lat_e7,
        lon_e7,
        alt_mm: alt_mm.into(),
        speed_mm_s: Natural::new(speed_mm_s),
        hdop_centi,
        sats,
        fix: asn1::FixKind::Fix3d,
    };
    Message {
        name: "gps_fix",
        title: "A GPS fix: signed integers, a bounded count, a four-way enum",
        rows: Encodings {
            plain: &value,
            postcard_tuned: Some(postcard_bytes(&postcard_value)),
            ww_plain: ww_bytes(&value),
            ww_tuned: ww_bytes(&tuned_value),
            protobuf: proto_value.encode_to_vec(),
            flatbuffers: planus::Builder::new().finish(&fb_value, None).to_vec(),
            asn1_closed: uper(&asn1_value),
            asn1_extensible: uper(&asn1_ext_value),
        }
        .into_rows(),
    }
}
