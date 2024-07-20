use wire_weaver_derive::ShrinkWrap;
use crate as wire_weaver;

#[derive(ShrinkWrap)]
pub struct Request {
    pub seq: u16,
    // path: Vec<vlu16n>,
    pub kind: RequestKind,
}

#[derive(ShrinkWrap)]
#[repr(u16)]
pub enum RequestKind {
    // Version { protocol_id: u32, version: Version },
    // Call { args: Vec<u8> },
    Call,
    // Read,
    // Write { value: Vec<u8> },
    // OpenStream,
    // CloseStream,
    // Subscribe,
    // Unsubscribe,
    // Borrow,
    // Release,
    // Introspect,
    Heartbeat,
}

#[derive(ShrinkWrap)]
pub struct Event {
    pub seq: u16,
    pub result: Result<EventKind, u8>,
}

#[derive(ShrinkWrap)]
#[repr(u16)]
pub enum EventKind {
    // Version { protocol_id: u32, version: Version },
    // ReturnValue { data: Vec<u8> },
    ReturnValue,
    // ReadValue { data: Vec<u8> },
    // Written,
    // StreamOpened,
    // TODO: Add Option<SizeHint>
    // StreamUpdate { data: Vec<u8> },
    // StreamClosed,
    // Subscribed,
    // RateChanged,
    // Unsubscribed,
    // Borrowed,
    // Released,
    // Introspect { ww_bytes: Vec<u8> },
    // Heartbeat { payload: () },
    Heartbeat,
}

#[cfg(test)]
mod tests {
    use shrink_wrap::{BufReader, BufWriter, ElementSize};
    use crate::client_server::{Request, RequestKind};

    #[test]
    fn sanity_check() {
        let req = Request {
            seq: 0,
            kind: RequestKind::Call,
        };
        let mut buf = [0u8; 8];
        let mut wr = BufWriter::new(&mut buf);
        wr.write(&req).unwrap();
        let bytes = wr.finish().unwrap();
        assert_eq!(bytes, &[0, 0, 0, 1]);
        let mut rd = BufReader::new(bytes);
        let req: Request = rd.read(ElementSize::Implied).unwrap();
    }
}