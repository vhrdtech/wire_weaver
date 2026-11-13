//! SRV-60: a firmware built without `ww_client_server`'s `multi` and `streams` features reads the requests they
//! hold as `RequestKind::Unknown` / `StreamSideband::Unknown` and answers `OperationNotSupported`, with the bytes of
//! the conformance corpus (`tests/conformance/corpus.json`, `client_server.request.*`), which are a host's.
//!
//! Ignored in a workspace run, where other crates turn the features on: `just test-lean-server` runs it alone
//! (`cargo nextest run -p lean_server --run-ignored only`).

use lean_server::reply;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    DeserializeShrinkWrap, SerializeShrinkWrap, UVlq32, UVlq32Backfill,
};
use ww_client_server::{
    Error, Event, EventKind, PathKind, Priority, Request, RequestKind, StreamSideband, err_seq,
};

/// The golden bytes of a corpus case.
fn corpus(name: &str) -> Vec<u8> {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../conformance/corpus.json")).unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no corpus case {name}"));
    hex::decode(case["bytes"].as_str().unwrap()).unwrap()
}

/// The bytes of an error answer to seq 1.
fn refused(err_seq: u32) -> Vec<u8> {
    let event = Event {
        seq: UVlq32(1),
        result: Err(Error::not_supported(err_seq)),
    };
    let mut buf = [0u8; 16];
    event.to_ww_bytes(&mut buf).unwrap().to_vec()
}

const LEAN: &str = "needs ww_client_server without multi and streams, which a workspace build turns on: just test-lean-server";

#[test]
#[ignore = "needs ww_client_server without multi and streams, which a workspace build turns on: just test-lean-server"]
fn multi_request_is_refused() {
    let request = corpus("client_server.request.multi_read");
    let kind = Request::from_ww_bytes(&request).unwrap().kind;
    assert!(matches!(kind, RequestKind::Unknown(3)), "{LEAN}: {kind:?}");
    let mut scratch = [0u8; 64];
    let answer = reply(&request, &mut scratch);
    assert_eq!(answer, refused(err_seq::REQUEST_KIND_UNKNOWN));
    // golden: seq 1, an error at request site 61
    assert_eq!(answer, [0x01, 0x0d, 0x70, 0x00, 0x01]);
}

#[test]
#[ignore = "needs ww_client_server without multi and streams, which a workspace build turns on: just test-lean-server"]
fn credit_sink_write_is_refused() {
    let request = corpus("client_server.request.sink_write");
    let kind = Request::from_ww_bytes(&request).unwrap().kind;
    assert!(matches!(kind, RequestKind::Unknown(10)), "{LEAN}: {kind:?}");
    let mut scratch = [0u8; 64];
    assert_eq!(
        reply(&request, &mut scratch),
        refused(err_seq::REQUEST_KIND_UNKNOWN)
    );
}

#[test]
#[ignore = "needs ww_client_server without multi and streams, which a workspace build turns on: just test-lean-server"]
fn stream_ack_is_refused_before_the_handler() {
    // base.s0, a plain stream: its sideband handler panics on an Unknown it is given
    let request = corpus("client_server.request.sideband_ack");
    let kind = Request::from_ww_bytes(&request).unwrap().kind;
    assert!(
        matches!(
            kind,
            RequestKind::StreamSideband {
                sideband: StreamSideband::Unknown(5)
            }
        ),
        "{LEAN}: {kind:?}"
    );
    let mut scratch = [0u8; 64];
    assert_eq!(
        reply(&request, &mut scratch),
        refused(err_seq::KIND_NOT_TAKEN)
    );
}

#[test]
#[ignore = "needs ww_client_server without multi and streams, which a workspace build turns on: just test-lean-server"]
fn the_rest_is_served() {
    // a call of base.c0 on the same server, answered as without the features
    let path = ww_client_server::resource_path([0, 0]);
    let request = Request {
        seq: UVlq32Backfill(1),
        priority: Priority::NORMAL.nibble(),
        path_kind: PathKind::absolute(&path),
        kind: RequestKind::Call {
            args: TailBytes(&[]),
        },
    };
    let mut buf = [0u8; 32];
    let request = request.to_ww_bytes(&mut buf).unwrap();
    let ok = Event {
        seq: UVlq32(1),
        result: Ok(EventKind::Value {
            data: TailBytes(&[]),
        }),
    };
    let mut ok_buf = [0u8; 16];
    let mut scratch = [0u8; 64];
    assert_eq!(
        reply(request, &mut scratch),
        ok.to_ww_bytes(&mut ok_buf).unwrap()
    );
}
