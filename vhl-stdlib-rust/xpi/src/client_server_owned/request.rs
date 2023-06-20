use super::{Error, Nrl, ReplyAck, TraitDescriptor};
use super::{Reply, ReplyKind};

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
    pub fn flip_with_error(&self, err: Error) -> Reply {
        let kind = match self.kind {
            RequestKind::Call { .. } => ReplyKind::CallResult {
                ret_value: Err(err),
            },
            RequestKind::Read => ReplyKind::ReadResult { value: Err(err) },
            RequestKind::Write { .. } => ReplyKind::WriteResult { status: Err(err) },
            RequestKind::OpenStream => ReplyKind::OpenStreamResult { status: Err(err) },
            RequestKind::CloseStream => ReplyKind::CloseStreamResult { status: Err(err) },
            RequestKind::Subscribe => ReplyKind::SubscribeResult { status: Err(err) },
            RequestKind::Unsubscribe => ReplyKind::UnsubscribeResult { status: Err(err) },
            RequestKind::Borrow => ReplyKind::BorrowResult { status: Err(err) },
            RequestKind::Release => ReplyKind::ReadResult { value: Err(err) },
            RequestKind::Introspect => ReplyKind::IntrospectResult { vhl: Err(err) },
            RequestKind::Ping => todo!(),
        };
        Reply {
            nrl: self.nrl.clone(),
            kind,
        }
    }
}
