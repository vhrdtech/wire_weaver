use core::fmt::Display;

use crate::error::XpiError;

use super::Nrl;
use strum::EnumDiscriminants;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Reply {
    pub nrl: Nrl,
    pub kind: Result<ReplyKind, XpiError>,
}

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

impl Display for Reply {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.kind {
            Ok(reply_kind) => match reply_kind {
                ReplyKind::ReturnValue { data } => write!(f, "ReturnValue{}({data:x?})", self.nrl),
                ReplyKind::ReadValue { data } => write!(f, "ReadValue{}({data:x?})", self.nrl),
                ReplyKind::Written => {
                    write!(f, "Written{}", self.nrl)
                }
                ReplyKind::StreamOpened => {
                    write!(f, "StreamOpened{}", self.nrl)
                }
                ReplyKind::StreamUpdate { data } => {
                    write!(f, "StreamUpdate{}({data:x?})", self.nrl)
                }
                ReplyKind::StreamClosed => {
                    write!(f, "StreamClosed{}", self.nrl)
                }
                ReplyKind::Subscribed => {
                    write!(f, "Subscribed{}", self.nrl)
                }
                ReplyKind::RateChanged => {
                    write!(f, "RateCRateChangedhangeResult{}", self.nrl)
                }
                ReplyKind::Unsubscribed => {
                    write!(f, "Unsubscribed{}", self.nrl)
                }
                ReplyKind::Borrowed => {
                    write!(f, "Borrowed{}", self.nrl)
                }
                ReplyKind::Released => {
                    write!(f, "Released{}", self.nrl)
                }
                ReplyKind::Introspect { vhl } => {
                    write!(f, "Introspect{}({vhl:x?})", self.nrl)
                }
                ReplyKind::Pong { payload } => write!(f, "Pong{}({payload:x?})", self.nrl),
            },
            Err(e) => write!(f, "{e}"),
        }
    }
}
