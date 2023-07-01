use core::fmt::Display;

use super::Error;
use super::Nrl;
use strum::EnumDiscriminants;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Reply {
    pub nrl: Nrl,
    pub kind: ReplyKind,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, EnumDiscriminants)]
pub enum ReplyKind {
    CallResult { ret_value: Result<Vec<u8>, Error> },
    ReadResult { value: Result<Vec<u8>, Error> },
    WriteResult { status: Result<(), Error> },
    OpenStreamResult { status: Result<(), Error> },
    StreamUpdate { data: Result<Vec<u8>, Error> },
    CloseStreamResult { status: Result<(), Error> },
    SubscribeResult { status: Result<(), Error> },
    RateChangeResult { status: Result<(), Error> },
    UnsubscribeResult { status: Result<(), Error> },
    BorrowResult { status: Result<(), Error> },
    ReleaseResult { status: Result<(), Error> },
    IntrospectResult { vhl: Result<Vec<u8>, Error> },
    Pong { payload: Result<(), Error> },
}

impl Display for Reply {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.kind {
            ReplyKind::CallResult { ret_value } => write!(f, "CallResult({ret_value:x?})"),
            ReplyKind::ReadResult { value } => write!(f, "ReadResult{}({value:x?})", self.nrl),
            ReplyKind::WriteResult { status } => write!(f, "WriteResult{}({status:x?})", self.nrl),
            ReplyKind::OpenStreamResult { status } => {
                write!(f, "OpenStreamResult{}({status:x?})", self.nrl)
            }
            ReplyKind::StreamUpdate { data } => write!(f, "StreamUpdate{}({data:x?})", self.nrl),
            ReplyKind::CloseStreamResult { status } => {
                write!(f, "CloseStreamResult{}({status:x?})", self.nrl)
            }
            ReplyKind::SubscribeResult { status } => {
                write!(f, "SubscribeResult{}({status:x?})", self.nrl)
            }
            ReplyKind::RateChangeResult { status } => {
                write!(f, "RateChangeResult{}({status:x?})", self.nrl)
            }
            ReplyKind::UnsubscribeResult { status } => {
                write!(f, "UnsubscribeResult{}({status:x?})", self.nrl)
            }
            ReplyKind::BorrowResult { status } => {
                write!(f, "BorrowResult{}({status:x?})", self.nrl)
            }
            ReplyKind::ReleaseResult { status } => {
                write!(f, "ReleaseResult{}({status:x?})", self.nrl)
            }
            ReplyKind::IntrospectResult { vhl } => {
                write!(f, "IntrospectResult{}({vhl:x?})", self.nrl)
            }
            ReplyKind::Pong { payload } => write!(f, "Pong{}({payload:x?})", self.nrl),
        }
    }
}
