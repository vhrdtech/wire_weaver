use core::fmt::Display;

// #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
// pub struct Request {
//     /// only for 'impl once <trait>'
//     pub tr: Option<TraitDescriptor>,
//     // pub nrl: Nrl,
//     pub reply_ack: ReplyAck,
//     pub kind: RequestKind,
// }

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

impl RequestKind {
    pub fn call(args: Vec<u8>) -> Self {
        RequestKind::Call { args }
    }
}

impl Display for RequestKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RequestKind::Call { args } => write!(f, "Call([{}B])", args.len()),
            RequestKind::Read => write!(f, "Read"),
            RequestKind::Write { value } => write!(f, "Write([{}B])", value.len()),
            RequestKind::OpenStream => write!(f, "OpenStream"),
            RequestKind::CloseStream => write!(f, "CloseStream"),
            RequestKind::Subscribe => write!(f, "Subscribe"),
            RequestKind::Unsubscribe => write!(f, "Unsubscribe"),
            RequestKind::Borrow => write!(f, "Borrow"),
            RequestKind::Release => write!(f, "Release"),
            RequestKind::Introspect => write!(f, "Introspect"),
            RequestKind::Ping => write!(f, "Ping"),
        }
    }
}
