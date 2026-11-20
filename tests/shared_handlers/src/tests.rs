use std::time::Duration;

use wire_weaver::Peer;
use wire_weaver::prelude::*;
use ww_client_server::{ErrorKind, Event, EventKind, err_seq};

use crate::async_server::AsyncServer;
use crate::blocking::Server;
use crate::requests::{self, Case};

/// Records every event sent through it, as both an async and a blocking [MessageSink].
#[derive(Default)]
struct Recorder(Vec<Vec<u8>>);

impl MessageSink for Recorder {
    async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.0.push(message.to_vec());
        Ok(())
    }
}

impl BlockingMessageSink for Recorder {
    fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.0.push(message.to_vec());
        Ok(())
    }
}

/// A reply and the events sent while answering it.
type Answer = (Vec<u8>, Vec<Vec<u8>>);

/// Cases a shared entry answers with an error on purpose, or not at all.
fn answered_ok(case: &Case) -> bool {
    !matches!(
        case.name,
        "channel[9] bad index" | "add_later (deferred)" | "add with seq 0"
    )
}

/// `reply` as an event: its value bytes, or the error's site and kind.
fn decode(reply: &[u8]) -> Result<Vec<u8>, (u32, String)> {
    let event = Event::from_ww_bytes(reply).expect("an event");
    match event.result {
        Ok(EventKind::Value { data }) => Ok(data.0.to_vec()),
        Ok(other) => panic!("not a value: {other:?}"),
        Err(e) => Err((e.err_seq(), format!("{:?}", e.kind()))),
    }
}

fn blocking(exclusive: bool, bytes: &[u8]) -> Answer {
    let mut srv = Server::default();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut out = EventWriter::new(Recorder::default(), &mut event_scratch);
    let reply = if exclusive {
        srv.process_request_bytes(bytes, &mut scratch, &mut out, ())
    } else {
        srv.process_shared_bytes(bytes, &mut scratch, &mut out, ())
    };
    (reply.expect("answered").to_vec(), out.sink.0)
}

async fn run_async(exclusive: bool, bytes: &[u8]) -> Answer {
    let mut srv = AsyncServer::default();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut out = EventWriter::new(Recorder::default(), &mut event_scratch);
    let reply = if exclusive {
        srv.process_request_bytes(bytes, &mut scratch, &mut out, ())
            .await
    } else {
        srv.process_shared_bytes(bytes, &mut scratch, &mut out, ())
            .await
    };
    (reply.expect("answered").to_vec(), out.sink.0)
}

/// A shared case answers the same through both entries, and with a value unless refused on purpose; an exclusive
/// one is refused by the shared entry (or not answered, for seq 0).
fn check(case: &Case, exclusive: Answer, shared: Answer) {
    if case.shared {
        assert_eq!(exclusive, shared, "{}", case.name);
        if answered_ok(case) {
            assert!(
                decode(&shared.0).is_ok(),
                "{}: {:?}",
                case.name,
                decode(&shared.0)
            );
        }
    } else if !shared.0.is_empty() && !case.name.starts_with("multi") {
        // a multi request is answered with a value holding an error per item
        assert!(
            decode(&shared.0).is_err(),
            "{}: shared entry served it",
            case.name
        );
    }
}

#[test]
fn is_shared_on_every_path_shape() {
    for case in requests::cases() {
        assert_eq!(Server::is_shared(&case.bytes), case.shared, "{}", case.name);
        assert_eq!(
            AsyncServer::is_shared(&case.bytes),
            case.shared,
            "{}",
            case.name
        );
    }
    assert!(
        !Server::is_shared(&[]),
        "a request that doesn't decode is not shared"
    );
}

#[test]
fn blocking_no_alloc_entries_agree() {
    for case in requests::cases() {
        check(
            &case,
            blocking(true, &case.bytes),
            blocking(false, &case.bytes),
        );
    }
}

#[tokio::test]
async fn async_alloc_entries_agree() {
    for case in requests::cases() {
        check(
            &case,
            run_async(true, &case.bytes).await,
            run_async(false, &case.bytes).await,
        );
    }
}

#[test]
fn shared_entry_values() {
    let value = |bytes: &[u8]| decode(&blocking(false, bytes).0).expect("a value");
    assert_eq!(u32::from_ww_bytes(&value(&requests::add(1))).unwrap(), 5);
    assert_eq!(
        u32::from_ww_bytes(&value(&requests::scaled(3))).unwrap(),
        33
    );
    let path = ww_client_server::resource_path([9, 1, 0]);
    assert_eq!(
        u32::from_ww_bytes(&value(&requests::read(1, &path))).unwrap(),
        7
    );
}

#[test]
fn shared_entry_refuses_an_exclusive_item() {
    let bump = requests::call(1, &ww_client_server::resource_path([2]), &[]);
    let (seq, kind) = decode(&blocking(false, &bump).0).unwrap_err();
    assert_eq!(seq, err_seq::REQUEST_NOT_SHARED);
    assert_eq!(kind, format!("{:?}", ErrorKind::OperationNotSupported));
}

#[test]
fn blocking_deferred_through_the_shared_entry() {
    let srv = Server::default();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut out = EventWriter::new(Recorder::default(), &mut event_scratch);
    let reply = srv
        .process_shared_bytes(&requests::add_later(), &mut scratch, &mut out, ())
        .unwrap();
    assert!(reply.is_empty(), "deferred: answered later");
    let (reply_to, sum) = srv.add_later.lock().unwrap().take().expect("kept");
    Server::add_later_send_return_blocking(&mut out, reply_to, sum).unwrap();
    let [event] = out.sink.0.as_slice() else {
        panic!("one event: {:?}", out.sink.0);
    };
    assert_eq!(u32::from_ww_bytes(&decode(event).unwrap()).unwrap(), 5);
}

#[tokio::test]
async fn async_deferred_through_the_shared_entry() {
    let srv = AsyncServer::default();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut out = EventWriter::new(Recorder::default(), &mut event_scratch);
    let reply = srv
        .process_shared_bytes(&requests::add_later(), &mut scratch, &mut out, ())
        .await
        .unwrap();
    assert!(reply.is_empty(), "deferred: answered later");
    let (reply_to, sum) = srv.add_later.lock().unwrap().take().expect("kept");
    AsyncServer::add_later_send_return(&mut out, reply_to, sum)
        .await
        .unwrap();
    let [event] = out.sink.0.as_slice() else {
        panic!("one event: {:?}", out.sink.0);
    };
    assert_eq!(u32::from_ww_bytes(&decode(event).unwrap()).unwrap(), 5);
}

/// `scaled(0)` waits for `scaled(3)` to open its gate: both finish only when they run side by side, through one
/// `&AsyncServer`.
#[tokio::test(start_paused = true)]
async fn shared_calls_run_side_by_side() {
    let srv = AsyncServer::default();
    let (waiting, opening) = (requests::scaled(0), requests::scaled(3));
    let (mut scratch_a, mut scratch_b) = ([0u8; 512], [0u8; 512]);
    let (mut events_a, mut events_b) = ([0u8; 512], [0u8; 512]);
    let mut out_a = EventWriter::new(Recorder::default(), &mut events_a);
    let mut out_b = EventWriter::new(Recorder::default(), &mut events_b);
    let both = async {
        tokio::join!(
            srv.process_shared_bytes(&waiting, &mut scratch_a, &mut out_a, ()),
            srv.process_shared_bytes(&opening, &mut scratch_b, &mut out_b, ()),
        )
    };
    let (a, b) = tokio::time::timeout(Duration::from_secs(5), both)
        .await
        .expect("the calls ran side by side");
    assert_eq!(u32::from_ww_bytes(&decode(a.unwrap()).unwrap()).unwrap(), 0);
    assert_eq!(
        u32::from_ww_bytes(&decode(b.unwrap()).unwrap()).unwrap(),
        33
    );
}

/// The `_from` entries hand the connection's origin to handlers, with the request's seq and priority; the plain
/// entries a peer the transport doesn't identify.
#[tokio::test]
async fn origin_reaches_handlers() {
    let mut srv = AsyncServer::default();
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut out = EventWriter::new(Recorder::default(), &mut event_scratch);
    let origin = CallContext::local();
    let request = requests::add(9);

    srv.process_shared_from(&origin, &request, &mut scratch, &mut out, ())
        .await
        .unwrap();
    let call = srv.add_call.lock().unwrap().take().unwrap();
    assert_eq!((call.peer, call.seq), (Peer::Local, 9));

    srv.process_request_from(&origin, &request, &mut scratch, &mut out, ())
        .await
        .unwrap();
    let call = srv.add_call.lock().unwrap().take().unwrap();
    assert_eq!((call.peer, call.seq), (Peer::Local, 9));

    srv.process_shared_bytes(&request, &mut scratch, &mut out, ())
        .await
        .unwrap();
    let call = srv.add_call.lock().unwrap().take().unwrap();
    assert_eq!((call.peer, call.seq), (Peer::Unknown, 9));
}

/// What a host runtime does with [WireWeaverSharedApiBackend]: ask, then serve through `&self` or `&mut self`.
async fn serve<B: WireWeaverSharedApiBackend<Medium = ()>>(srv: &mut B, request: &[u8]) -> Vec<u8> {
    let mut scratch = [0u8; 512];
    let mut event_scratch = [0u8; 512];
    let mut out = EventWriter::new(Recorder::default(), &mut event_scratch);
    let reply = if B::is_shared(request) {
        srv.process_shared_bytes(&CallContext::local(), &mut out, (), request, &mut scratch)
            .await
    } else {
        srv.process_bytes(&mut out, (), request, &mut scratch).await
    };
    reply.unwrap().to_vec()
}

#[tokio::test]
async fn backend_trait_routes_requests() {
    let mut srv = AsyncServer::default();
    let counter = requests::read(1, &ww_client_server::resource_path([4]));
    let bump = requests::call(1, &ww_client_server::resource_path([2]), &[]);
    let read = |reply: Vec<u8>| u32::from_ww_bytes(&decode(&reply).unwrap()).unwrap();
    assert_eq!(read(serve(&mut srv, &counter).await), 4);
    serve(&mut srv, &bump).await;
    assert_eq!(read(serve(&mut srv, &counter).await), 5);
}
