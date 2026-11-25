//! A state report made of enums: a mode, a link state with payloads, a list of faults.

use crate::asn1::{Natural, uper};
use crate::fbs_messages::wire_sizes as fb;
use crate::rows::{Encodings, Message, ww_bytes_owned};
use prost::Message as _;
use wire_weaver::prelude::*;

mod plain {
    use super::*;
    use serde::Serialize;

    #[derive_shrink_wrap(owned, derive(Serialize), ww_repr = u8)]
    pub enum ModeOwned {
        Idle,
        Init,
        Running,
        Paused,
        Fault,
    }

    #[derive_shrink_wrap(owned, derive(Serialize), ww_repr = u8)]
    pub enum FaultOwned {
        None,
        Overcurrent,
        Overtemp,
        Undervoltage,
        SensorLost,
        LinkLost,
        FlashEcc,
        Watchdog,
    }

    #[derive_shrink_wrap(owned, derive(Serialize), ww_repr = u8)]
    pub enum LinkStateOwned {
        Down,
        Up { mbps: u16 },
        Error { code: u8 },
    }

    #[derive_shrink_wrap(derive(Serialize))]
    pub struct StateReportOwned {
        pub mode: ModeOwned,
        pub link: LinkStateOwned,
        pub faults: Vec<FaultOwned>,
        pub battery_pct: u8,
        pub charging: bool,
        pub uptime_s: u32,
    }
}

mod tuned {
    use super::*;

    #[derive_shrink_wrap(owned, ww_repr = u3, sized)]
    pub enum ModeOwned {
        Idle,
        Init,
        Running,
        Paused,
        Fault,
    }

    #[derive_shrink_wrap(owned, ww_repr = u3, sized)]
    pub enum FaultOwned {
        None,
        Overcurrent,
        Overtemp,
        Undervoltage,
        SensorLost,
        LinkLost,
        FlashEcc,
        Watchdog,
    }

    #[derive_shrink_wrap(owned, ww_repr = u2, self_describing)]
    pub enum LinkStateOwned {
        Down,
        Up {
            #[wire(UNib32)]
            mbps: u16,
        },
        Error {
            code: u8,
        },
    }

    #[derive_shrink_wrap]
    pub struct StateReportOwned {
        pub mode: ModeOwned,
        pub link: LinkStateOwned,
        pub faults: Vec<FaultOwned>,
        #[wire(U7)]
        pub battery_pct: u8,
        pub charging: bool,
        #[wire(UNib32)]
        pub uptime_s: u32,
    }
}

mod proto {
    use prost::{Enumeration, Message, Oneof};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
    #[repr(i32)]
    pub enum Mode {
        Idle = 0,
        Init = 1,
        Running = 2,
        Paused = 3,
        Fault = 4,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
    #[repr(i32)]
    pub enum Fault {
        None = 0,
        Overcurrent = 1,
        Overtemp = 2,
        Undervoltage = 3,
        SensorLost = 4,
        LinkLost = 5,
        FlashEcc = 6,
        Watchdog = 7,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct LinkDown {}

    #[derive(Clone, PartialEq, Message)]
    pub struct LinkUp {
        #[prost(uint32, tag = "1")]
        pub mbps: u32,
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct LinkError {
        #[prost(uint32, tag = "1")]
        pub code: u32,
    }

    #[derive(Clone, PartialEq, Oneof)]
    pub enum LinkState {
        #[prost(message, tag = "2")]
        Down(LinkDown),
        #[prost(message, tag = "3")]
        Up(LinkUp),
        #[prost(message, tag = "4")]
        Error(LinkError),
    }

    #[derive(Clone, PartialEq, Message)]
    pub struct StateReport {
        #[prost(enumeration = "Mode", tag = "1")]
        pub mode: i32,
        #[prost(oneof = "LinkState", tags = "2,3,4")]
        pub link: Option<LinkState>,
        #[prost(enumeration = "Fault", repeated, packed = "true", tag = "5")]
        pub faults: Vec<i32>,
        #[prost(uint32, tag = "6")]
        pub battery_pct: u32,
        #[prost(bool, tag = "7")]
        pub charging: bool,
        #[prost(uint32, tag = "8")]
        pub uptime_s: u32,
    }
}

mod asn1 {
    use crate::asn1::{Natural, asn1_pair};
    use rasn::prelude::*;

    #[derive(AsnType, Encode, Decode, Clone, Copy, PartialEq, Debug)]
    #[rasn(enumerated)]
    pub enum Mode {
        Idle,
        Init,
        Running,
        Paused,
        Fault,
    }

    #[derive(AsnType, Encode, Decode, Clone, Copy, PartialEq, Debug)]
    #[rasn(enumerated)]
    pub enum Fault {
        None,
        Overcurrent,
        Overtemp,
        Undervoltage,
        SensorLost,
        LinkLost,
        FlashEcc,
        Watchdog,
    }

    #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
    #[rasn(choice, automatic_tags)]
    pub enum LinkState {
        Down(()),
        Up(u16),
        Error(u8),
    }

    asn1_pair! {
        StateReport, StateReportExtensible {
            pub mode: Mode,
            pub link: LinkState,
            pub faults: SequenceOf<Fault>,
            #[rasn(value("0..=100"))]
            pub battery_pct: u8,
            pub charging: bool,
            pub uptime_s: Natural,
        }
    }
}

pub fn message() -> Message {
    let (battery_pct, charging, uptime_s) = (87, true, 93_784);

    let value = plain::StateReportOwned {
        mode: plain::ModeOwned::Running,
        link: plain::LinkStateOwned::Up { mbps: 480 },
        faults: vec![plain::FaultOwned::Overtemp, plain::FaultOwned::LinkLost],
        battery_pct,
        charging,
        uptime_s,
    };
    let tuned_value = tuned::StateReportOwned {
        mode: tuned::ModeOwned::Running,
        link: tuned::LinkStateOwned::Up { mbps: 480 },
        faults: vec![tuned::FaultOwned::Overtemp, tuned::FaultOwned::LinkLost],
        battery_pct,
        charging,
        uptime_s,
    };
    let proto_value = proto::StateReport {
        mode: proto::Mode::Running as i32,
        link: Some(proto::LinkState::Up(proto::LinkUp { mbps: 480 })),
        faults: vec![proto::Fault::Overtemp as i32, proto::Fault::LinkLost as i32],
        battery_pct: battery_pct.into(),
        charging,
        uptime_s,
    };
    let fb_value = fb::StateReport {
        mode: fb::Mode::Running,
        link: Some(fb::LinkState::LinkUp(Box::new(fb::LinkUp { mbps: 480 }))),
        faults: Some(vec![fb::Fault::Overtemp, fb::Fault::LinkLost]),
        battery_pct,
        charging,
        uptime_s,
    };
    let asn1_value = asn1::StateReport {
        mode: asn1::Mode::Running,
        link: asn1::LinkState::Up(480),
        faults: vec![asn1::Fault::Overtemp, asn1::Fault::LinkLost],
        battery_pct,
        charging,
        uptime_s: Natural::new(uptime_s),
    };
    let asn1_ext_value = asn1::StateReportExtensible {
        mode: asn1::Mode::Running,
        link: asn1::LinkState::Up(480),
        faults: vec![asn1::Fault::Overtemp, asn1::Fault::LinkLost],
        battery_pct,
        charging,
        uptime_s: Natural::new(uptime_s),
    };
    Message {
        name: "state_report",
        title: "A state report made of enums",
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
