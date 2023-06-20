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
    Pong { payload: () },
}
