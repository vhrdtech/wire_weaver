//! An inertial sensor sample: mostly `f32`, the case where a dense format has little to save.

use crate::asn1::{Natural, uper};
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, ww_bytes};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(borrowed, derive(Serialize))]
    pub struct Telemetry {
        pub seq: u16,
        pub t_us: u64,
        pub accel: [f32; 3],
        pub gyro: [f32; 3],
        pub temp_c: f32,
        pub battery_mv: u16,
    }
}

mod tuned {
    use super::*;

    #[derive_shrink_wrap(borrowed)]
    pub struct Telemetry {
        pub seq: u16,
        #[wire(UVlq64)]
        pub t_us: u64,
        pub accel: [f32; 3],
        pub gyro: [f32; 3],
        pub temp_c: f32,
        #[wire(U12)]
        pub battery_mv: u16,
    }
}

mod proto {
    use prost::Message;

    #[derive(Clone, PartialEq, Message)]
    pub struct Telemetry {
        #[prost(uint32, tag = "1")]
        pub seq: u32,
        #[prost(uint64, tag = "2")]
        pub t_us: u64,
        #[prost(float, repeated, tag = "3")]
        pub accel: Vec<f32>,
        #[prost(float, repeated, tag = "4")]
        pub gyro: Vec<f32>,
        #[prost(float, tag = "5")]
        pub temp_c: f32,
        #[prost(uint32, tag = "6")]
        pub battery_mv: u32,
    }
}

/// rasn's UPER codec cannot encode REAL, so a float travels as its four IEEE 754 bytes in an OCTET STRING (SIZE(4)).
mod asn1 {
    use crate::asn1::{Natural, asn1_pair};
    use rasn::prelude::*;

    pub fn real(value: f32) -> FixedOctetString<4> {
        FixedOctetString::new(value.to_be_bytes())
    }

    asn1_pair! {
        Telemetry, TelemetryExtensible {
            pub seq: u16,
            pub t_us: Natural,
            #[rasn(size("3"))]
            pub accel: SequenceOf<FixedOctetString<4>>,
            #[rasn(size("3"))]
            pub gyro: SequenceOf<FixedOctetString<4>>,
            pub temp_c: FixedOctetString<4>,
            #[rasn(value("0..=4095"))]
            pub battery_mv: u16,
        }
    }
}

pub fn message() -> Message {
    let (seq, t_us, battery_mv) = (4821, 1_234_567_890, 3712);
    let accel = [0.012, -0.034, 9.806];
    let gyro = [0.0012, 0.0004, -0.0031];
    let temp_c = 31.25;

    let value = plain::Telemetry {
        seq,
        t_us,
        accel,
        gyro,
        temp_c,
        battery_mv,
    };
    let tuned_value = tuned::Telemetry {
        seq,
        t_us,
        accel,
        gyro,
        temp_c,
        battery_mv,
    };
    let proto_value = proto::Telemetry {
        seq: seq.into(),
        t_us,
        accel: accel.to_vec(),
        gyro: gyro.to_vec(),
        temp_c,
        battery_mv: battery_mv.into(),
    };
    let fb_value = fb::Telemetry {
        seq,
        t_us,
        accel: Some(fb::Vec3 {
            x: accel[0],
            y: accel[1],
            z: accel[2],
        }),
        gyro: Some(fb::Vec3 {
            x: gyro[0],
            y: gyro[1],
            z: gyro[2],
        }),
        temp_c,
        battery_mv,
    };
    let asn1_value = asn1::Telemetry {
        seq,
        t_us: Natural::new(t_us),
        accel: accel.map(asn1::real).to_vec(),
        gyro: gyro.map(asn1::real).to_vec(),
        temp_c: asn1::real(temp_c),
        battery_mv,
    };
    let asn1_ext_value = asn1::TelemetryExtensible {
        seq,
        t_us: Natural::new(t_us),
        accel: accel.map(asn1::real).to_vec(),
        gyro: gyro.map(asn1::real).to_vec(),
        temp_c: asn1::real(temp_c),
        battery_mv,
    };
    Message {
        name: "telemetry",
        title: "An inertial sensor sample: mostly f32",
        rows: Encodings {
            plain: &value,
            postcard_tuned: None,
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
