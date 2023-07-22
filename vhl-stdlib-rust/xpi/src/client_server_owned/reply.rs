use core::fmt::Display;

use strum::EnumDiscriminants;

// #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
// pub struct Reply {
//     // pub nrl: Nrl,
//     pub kind: Result<ReplyKind, XpiError>,
// }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, EnumDiscriminants)]
pub enum ReplyKind {
    ReturnValue { data: Vec<u8> },
    ReadValue { data: Vec<u8> },
    Written,
    StreamOpened,
    StreamUpdate { data: Vec<u8> },
    StreamClosed,
    Subscribed,
    RateChanged,
    Unsubscribed,
    Borrowed,
    Released,
    Introspect { vhl: Vec<u8> },
    Pong { payload: () },
}

impl Display for ReplyKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReplyKind::ReturnValue { data } => write!(f, "ReturnValue({data:x?})"),
            ReplyKind::ReadValue { data } => write!(f, "ReadValue({data:x?})"),
            ReplyKind::Written => {
                write!(f, "Written")
            }
            ReplyKind::StreamOpened => {
                write!(f, "StreamOpened")
            }
            ReplyKind::StreamUpdate { data } => {
                write!(f, "StreamUpdate({data:x?})")
            }
            ReplyKind::StreamClosed => {
                write!(f, "StreamClosed")
            }
            ReplyKind::Subscribed => {
                write!(f, "Subscribed")
            }
            ReplyKind::RateChanged => {
                write!(f, "RateCRateChangedhangeResult")
            }
            ReplyKind::Unsubscribed => {
                write!(f, "Unsubscribed")
            }
            ReplyKind::Borrowed => {
                write!(f, "Borrowed")
            }
            ReplyKind::Released => {
                write!(f, "Released")
            }
            ReplyKind::Introspect { vhl } => {
                write!(f, "Introspect({vhl:x?})")
            }
            ReplyKind::Pong { payload } => write!(f, "Pong({payload:x?})"),
        }
    }
}
