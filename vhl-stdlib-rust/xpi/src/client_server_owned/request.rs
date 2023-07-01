use core::fmt::Display;

use crate::error::XpiError;

use super::Reply;
use super::{Nrl, ReplyAck, TraitDescriptor};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Request {
    /// only for 'impl once <trait>'
    pub tr: Option<TraitDescriptor>,
    pub nrl: Nrl,
    pub reply_ack: ReplyAck,
    pub kind: RequestKind,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RequestKind {
    Call { args: Vec<u8> },
    Read,
    Write { value: Vec<u8> },
    OpenStream,
    CloseStream,
    Subscribe,
    Unsubscribe,
    Borrow,
    Release,
    Introspect,
    Ping,
}

impl Request {
    pub fn call(nrl: Nrl, args: Vec<u8>) -> Self {
        Request {
            tr: None,
            nrl,
            reply_ack: ReplyAck::Ack,
            kind: RequestKind::Call { args },
        }
    }

    pub fn flip_with_error(&self, err: XpiError) -> Reply {
        // let kind = match self.kind {
        //     RequestKind::Call { .. } => ReplyKind::CallResult {
        //         ret_value: Err(err),
        //     },
        //     RequestKind::Read => ReplyKind::ReadResult { value: Err(err) },
        //     RequestKind::Write { .. } => ReplyKind::WriteResult { status: Err(err) },
        //     RequestKind::OpenStream => ReplyKind::OpenStreamResult { status: Err(err) },
        //     RequestKind::CloseStream => ReplyKind::CloseStreamResult { status: Err(err) },
        //     RequestKind::Subscribe => ReplyKind::SubscribeResult { status: Err(err) },
        //     RequestKind::Unsubscribe => ReplyKind::UnsubscribeResult { status: Err(err) },
        //     RequestKind::Borrow => ReplyKind::BorrowResult { status: Err(err) },
        //     RequestKind::Release => ReplyKind::ReadResult { value: Err(err) },
        //     RequestKind::Introspect => ReplyKind::IntrospectResult { vhl: Err(err) },
        //     RequestKind::Ping => ReplyKind::Pong { payload: Err(err) },
        // };
        Reply {
            nrl: self.nrl.clone(),
            kind: Err(err),
        }
    }
}

impl Display for Request {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.kind {
            RequestKind::Call { args } => write!(f, "Call{}({args:x?})", self.nrl),
            RequestKind::Read => write!(f, "Read{}", self.nrl),
            RequestKind::Write { value } => write!(f, "Write{}({value:x?})", self.nrl),
            RequestKind::OpenStream => write!(f, "OpenStream{}", self.nrl),
            RequestKind::CloseStream => write!(f, "CloseStream{}", self.nrl),
            RequestKind::Subscribe => write!(f, "Subscribe{}", self.nrl),
            RequestKind::Unsubscribe => write!(f, "Unsubscribe{}", self.nrl),
            RequestKind::Borrow => write!(f, "Borrow{}", self.nrl),
            RequestKind::Release => write!(f, "Release{}", self.nrl),
            RequestKind::Introspect => write!(f, "Introspect{}", self.nrl),
            RequestKind::Ping => write!(f, "Ping{}", self.nrl),
        }
    }
}