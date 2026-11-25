//! A device configuration with many optional fields, a few of them set.

use crate::asn1::uper;
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, ww_bytes_owned};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct DeviceConfigOwned {
        pub name: Option<String>,
        pub baud: Option<u32>,
        pub address: Option<u8>,
        pub retries: Option<u8>,
        pub timeout_ms: Option<u16>,
        pub enabled: Option<bool>,
        pub gain: Option<f32>,
        pub label: Option<String>,
    }
}

/// `sparse`: a presence header, then only the fields that are set.
mod tuned {
    use super::*;

    #[derive_shrink_wrap(sparse)]
    pub struct DeviceConfigOwned {
        pub name: Option<String>,
        pub baud: Option<u32>,
        pub address: Option<u8>,
        pub retries: Option<u8>,
        pub timeout_ms: Option<u16>,
        pub enabled: Option<bool>,
        pub gain: Option<f32>,
        pub label: Option<String>,
    }
}

mod proto {
    use prost::Message;

    #[derive(Clone, PartialEq, Message)]
    pub struct DeviceConfig {
        #[prost(string, optional, tag = "1")]
        pub name: Option<String>,
        #[prost(uint32, optional, tag = "2")]
        pub baud: Option<u32>,
        #[prost(uint32, optional, tag = "3")]
        pub address: Option<u32>,
        #[prost(uint32, optional, tag = "4")]
        pub retries: Option<u32>,
        #[prost(uint32, optional, tag = "5")]
        pub timeout_ms: Option<u32>,
        #[prost(bool, optional, tag = "6")]
        pub enabled: Option<bool>,
        #[prost(float, optional, tag = "7")]
        pub gain: Option<f32>,
        #[prost(string, optional, tag = "8")]
        pub label: Option<String>,
    }
}

/// rasn's UPER codec cannot encode REAL, so a float travels as its four IEEE 754 bytes in an OCTET STRING (SIZE(4)).
mod asn1 {
    use crate::asn1::asn1_pair;
    use rasn::prelude::*;

    asn1_pair! {
        DeviceConfig, DeviceConfigExtensible {
            pub name: Option<Utf8String>,
            pub baud: Option<u32>,
            pub address: Option<u8>,
            pub retries: Option<u8>,
            pub timeout_ms: Option<u16>,
            pub enabled: Option<bool>,
            pub gain: Option<FixedOctetString<4>>,
            pub label: Option<Utf8String>,
        }
    }
}

pub fn message() -> Message {
    let name = Some("bench-3".to_string());
    let (baud, timeout_ms, enabled) = (Some(115_200), Some(250), Some(true));

    let value = plain::DeviceConfigOwned {
        name: name.clone(),
        baud,
        address: None,
        retries: None,
        timeout_ms,
        enabled,
        gain: None,
        label: None,
    };
    let tuned_value = tuned::DeviceConfigOwned {
        name: name.clone(),
        baud,
        address: None,
        retries: None,
        timeout_ms,
        enabled,
        gain: None,
        label: None,
    };
    let proto_value = proto::DeviceConfig {
        name: name.clone(),
        baud,
        address: None,
        retries: None,
        timeout_ms: timeout_ms.map(u32::from),
        enabled,
        gain: None,
        label: None,
    };
    let fb_value = fb::DeviceConfig {
        name: name.clone(),
        baud,
        address: None,
        retries: None,
        timeout_ms,
        enabled,
        gain: None,
        label: None,
    };
    let asn1_value = asn1::DeviceConfig {
        name: name.clone(),
        baud,
        address: None,
        retries: None,
        timeout_ms,
        enabled,
        gain: None,
        label: None,
    };
    let asn1_ext_value = asn1::DeviceConfigExtensible {
        name,
        baud,
        address: None,
        retries: None,
        timeout_ms,
        enabled,
        gain: None,
        label: None,
    };
    Message {
        name: "device_config",
        title: "A device configuration: eight optional fields, four set",
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
