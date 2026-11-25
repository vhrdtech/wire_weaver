//! A log record: mostly text, which no format can shrink.

use crate::asn1::{Natural, uper};
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, ww_bytes_owned};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(owned, derive(Serialize), ww_repr = u8)]
    pub enum LevelOwned {
        Trace,
        Debug,
        Info,
        Warn,
        Error,
    }

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct LogRecordOwned {
        pub t_ms: u64,
        pub level: LevelOwned,
        pub target: String,
        pub message: String,
        pub line: u32,
    }
}

mod tuned {
    use super::*;

    #[derive_shrink_wrap(owned, ww_repr = u3, sized)]
    pub enum LevelOwned {
        Trace,
        Debug,
        Info,
        Warn,
        Error,
    }

    #[derive_shrink_wrap]
    pub struct LogRecordOwned {
        #[wire(UVlq64)]
        pub t_ms: u64,
        pub level: LevelOwned,
        pub target: String,
        pub message: String,
        #[wire(UNib32)]
        pub line: u32,
    }
}

mod proto {
    use prost::{Enumeration, Message};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
    #[repr(i32)]
    pub enum Level {
        Trace = 0,
        Debug = 1,
        Info = 2,
        Warn = 3,
        Error = 4,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct LogRecord {
        #[prost(uint64, tag = "1")]
        pub t_ms: u64,
        #[prost(enumeration = "Level", tag = "2")]
        pub level: i32,
        #[prost(string, tag = "3")]
        pub target: String,
        #[prost(string, tag = "4")]
        pub message: String,
        #[prost(uint32, tag = "5")]
        pub line: u32,
    }
}

mod asn1 {
    use crate::asn1::{Natural, asn1_pair};
    use rasn::prelude::*;

    #[derive(AsnType, Encode, Decode, Clone, Copy, PartialEq, Debug)]
    #[rasn(enumerated)]
    pub enum Level {
        Trace,
        Debug,
        Info,
        Warn,
        Error,
    }

    asn1_pair! {
        LogRecord, LogRecordExtensible {
            pub t_ms: Natural,
            pub level: Level,
            pub target: Utf8String,
            pub message: Utf8String,
            pub line: Natural,
        }
    }
}

pub fn message() -> Message {
    let t_ms = 1_764_000_123_456;
    let target = "ww_device::server".to_string();
    let message = "link up: usb, 480 Mbit/s, 3 endpoints".to_string();
    let line = 412;

    let value = plain::LogRecordOwned {
        t_ms,
        level: plain::LevelOwned::Info,
        target: target.clone(),
        message: message.clone(),
        line,
    };
    let tuned_value = tuned::LogRecordOwned {
        t_ms,
        level: tuned::LevelOwned::Info,
        target: target.clone(),
        message: message.clone(),
        line,
    };
    let proto_value = proto::LogRecord {
        t_ms,
        level: proto::Level::Info as i32,
        target: target.clone(),
        message: message.clone(),
        line,
    };
    let fb_value = fb::LogRecord {
        t_ms,
        level: fb::LogLevel::Info,
        target: Some(target.clone()),
        message: Some(message.clone()),
        line,
    };
    let asn1_value = asn1::LogRecord {
        t_ms: Natural::new(t_ms),
        level: asn1::Level::Info,
        target: target.clone(),
        message: message.clone(),
        line: Natural::new(line),
    };
    let asn1_ext_value = asn1::LogRecordExtensible {
        t_ms: Natural::new(t_ms),
        level: asn1::Level::Info,
        target,
        message,
        line: Natural::new(line),
    };
    Message {
        name: "log_record",
        title: "A log record: mostly text",
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
