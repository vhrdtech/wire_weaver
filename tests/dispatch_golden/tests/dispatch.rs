//! TEST-31: every case of `corpus.json` against the sync and the async server (they must agree), and the frozen
//! golden bytes guard. `DISPATCH_GOLDEN=update cargo nextest run -p dispatch_golden corpus` fills in bytes still
//! pending (`"?"`); see `docs/serdes/conformance.md` for the pattern this follows.

use dispatch_golden::corpus::{self, Case, Corpus, Step};
use dispatch_golden::{async_server, frozen, requests, server};

/// Records every event sent through it, as both an async and a blocking [wire_weaver::MessageSink].
#[derive(Default)]
struct Recorder(Vec<Vec<u8>>);

impl wire_weaver::MessageSink for Recorder {
    async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.0.push(message.to_vec());
        Ok(())
    }
}

impl wire_weaver::BlockingMessageSink for Recorder {
    fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.0.push(message.to_vec());
        Ok(())
    }
}

/// Every step of `case` against a fresh sync server: the direct reply and the events queued while answering it.
fn run_case_sync(case: &Case) -> Vec<(Vec<u8>, Vec<Vec<u8>>)> {
    let mut srv = server::Server::default();
    (0..case.steps.len())
        .map(|i| {
            let request = requests::build(&case.name, i);
            let mut scratch = [0u8; 512];
            let mut event_scratch = [0u8; 512];
            let mut out = wire_weaver::EventWriter::new(Recorder::default(), &mut event_scratch);
            let reply = srv
                .process_request_bytes(&request, &mut scratch, &mut out, ())
                .unwrap_or_else(|e| panic!("case '{}' step {i}: {e:?}", case.name))
                .to_vec();
            (reply, out.sink.0)
        })
        .collect()
}

/// Same as [run_case_sync], against a fresh async server.
fn run_case_async(case: &Case) -> Vec<(Vec<u8>, Vec<Vec<u8>>)> {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let mut srv = async_server::AsyncServer::default();
        let mut results = vec![];
        for i in 0..case.steps.len() {
            let request = requests::build(&case.name, i);
            let mut scratch = [0u8; 512];
            let mut event_scratch = [0u8; 512];
            let mut out = wire_weaver::EventWriter::new(Recorder::default(), &mut event_scratch);
            let reply = srv
                .process_request_bytes(&request, &mut scratch, &mut out, ())
                .await
                .unwrap_or_else(|e| panic!("case '{}' step {i}: {e:?}", case.name))
                .to_vec();
            results.push((reply, out.sink.0));
        }
        results
    })
}

fn hex_list(bytes: &[Vec<u8>]) -> Vec<String> {
    bytes.iter().map(hex::encode).collect()
}

/// Every case against both servers: the sync server is the reference for golden bytes (matched against the
/// async server too), request bytes are rebuilt fresh from [requests::build] every run.
#[test]
fn corpus() {
    let loaded = corpus::load();
    let update = std::env::var("DISPATCH_GOLDEN").as_deref() == Ok("update");
    let mut failures = vec![];
    let mut new_cases = Vec::with_capacity(loaded.cases.len());

    for case in &loaded.cases {
        let sync_results = run_case_sync(case);
        let async_results = run_case_async(case);
        let mut new_steps = Vec::with_capacity(case.steps.len());

        for (i, step) in case.steps.iter().enumerate() {
            let request = requests::build(&case.name, i);
            let (sync_reply, sync_events) = &sync_results[i];
            let (async_reply, async_events) = &async_results[i];
            if sync_reply != async_reply {
                failures.push(format!(
                    "case '{}' step {i}: sync and async servers disagree on the reply: sync {}, async {}",
                    case.name,
                    hex::encode(sync_reply),
                    hex::encode(async_reply)
                ));
            }
            if sync_events != async_events {
                failures.push(format!(
                    "case '{}' step {i}: sync and async servers disagree on the queued events",
                    case.name
                ));
            }

            let request_hex = if step.request_to_fill() {
                if !update {
                    failures.push(format!(
                        "case '{}' step {i}: request has no golden bytes yet: just dispatch-golden-update",
                        case.name
                    ));
                }
                hex::encode(&request)
            } else {
                let golden = step.request_bytes();
                if golden != request {
                    failures.push(format!(
                        "case '{}' step {i}, request: {}",
                        case.name,
                        corpus::bytes_mismatch(&request, &golden)
                    ));
                }
                step.request.clone()
            };

            let reply_hex = if step.reply_to_fill() {
                if !update {
                    failures.push(format!(
                        "case '{}' step {i}: reply has no golden bytes yet: just dispatch-golden-update",
                        case.name
                    ));
                }
                hex::encode(sync_reply)
            } else {
                let golden = step.reply_bytes();
                if &golden != sync_reply {
                    failures.push(format!(
                        "case '{}' step {i}, reply: {}",
                        case.name,
                        corpus::bytes_mismatch(sync_reply, &golden)
                    ));
                }
                step.reply.clone()
            };

            if step.events.len() != sync_events.len() && !update {
                failures.push(format!(
                    "case '{}' step {i}: {} event(s) golden, {} produced",
                    case.name,
                    step.events.len(),
                    sync_events.len()
                ));
            }
            let events_hex: Vec<String> = if update {
                hex_list(sync_events)
            } else {
                step.events
                    .iter()
                    .enumerate()
                    .map(|(j, golden_hex)| {
                        let produced = sync_events.get(j).cloned().unwrap_or_default();
                        if golden_hex == "?" {
                            failures.push(format!(
                                "case '{}' step {i} event {j}: no golden bytes yet: just dispatch-golden-update",
                                case.name
                            ));
                        } else {
                            let golden = hex::decode(
                                golden_hex.split_whitespace().collect::<String>(),
                            )
                            .unwrap_or_else(|e| panic!("case '{}' event {j} is not hex: {e}", case.name));
                            if golden != produced {
                                failures.push(format!(
                                    "case '{}' step {i} event {j}: {}",
                                    case.name,
                                    corpus::bytes_mismatch(&produced, &golden)
                                ));
                            }
                        }
                        golden_hex.clone()
                    })
                    .collect()
            };

            new_steps.push(Step {
                request: request_hex,
                reply: reply_hex,
                events: events_hex,
            });
        }

        new_cases.push(Case {
            name: case.name.clone(),
            steps: new_steps,
            note: case.note.clone(),
            wire_change: case.wire_change.clone(),
        });
    }

    if update {
        let filled = Corpus {
            about: loaded.about,
            cases: new_cases,
        };
        let text = serde_json::to_string_pretty(&filled).expect("serialize corpus.json");
        std::fs::write(corpus::path(), format!("{text}\n")).expect("write corpus.json");
    } else {
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

/// TEST-24-style guard: a sealed step's `request`, `reply` or `events` don't change, and a case doesn't disappear,
/// once they are in the corpus at the merge base with main.
#[test]
fn frozen_bytes() {
    match frozen::baseline() {
        frozen::Baseline::Unavailable(reason) => {
            eprintln!("{reason}");
        }
        frozen::Baseline::Corrupt(reason) => panic!("{reason}"),
        frozen::Baseline::At { sha, corpus: old } => {
            let now = corpus::load();
            let failures = frozen::check(&old, &now, &sha);
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
    }
}

/// Multi requests whose `MultiArgs` holds an empty list, as the borrowed writer wrote them before EVO-81 (old) and
/// writes them since (new), with the case's name: MultiCall or MultiWrite, path `[10]` or `[10, 1]`, `Same` or
/// `Different`, with or without `in_each_array_id`. The old bytes are the output of the pre-fix commit
/// (`cf822c4e9^`) for these values.
const MULTI_EMPTY_ARGS: &[(&str, &str, &str)] = &[
    (
        "call.p1.same.each",
        "808080800140a11040128000000221",
        "808080800140a110401280000121",
    ),
    (
        "write.p1.same.each",
        "808080800140a15040128000000221",
        "808080800140a150401280000121",
    ),
    (
        "call.p1.same.none",
        "808080800140a11040120000000221",
        "808080800140a110401200000121",
    ),
    (
        "write.p1.same.none",
        "808080800140a15040120000000221",
        "808080800140a150401200000121",
    ),
    (
        "call.p1.diff.each",
        "808080800140a11040128080000221",
        "808080800140a110401280800121",
    ),
    (
        "write.p1.diff.each",
        "808080800140a15040128080000221",
        "808080800140a150401280800121",
    ),
    (
        "call.p1.diff.none",
        "808080800140a11040120080000221",
        "808080800140a110401200800121",
    ),
    (
        "write.p1.diff.none",
        "808080800140a15040120080000221",
        "808080800140a150401200800121",
    ),
    (
        "call.p2.same.each",
        "808080800140a11140128000000222",
        "808080800140a111401280000122",
    ),
    (
        "write.p2.same.each",
        "808080800140a11540128000000222",
        "808080800140a115401280000122",
    ),
    (
        "call.p2.same.none",
        "808080800140a11140120000000222",
        "808080800140a111401200000122",
    ),
    (
        "write.p2.same.none",
        "808080800140a11540120000000222",
        "808080800140a115401200000122",
    ),
    (
        "call.p2.diff.each",
        "808080800140a11140128080000222",
        "808080800140a111401280800122",
    ),
    (
        "write.p2.diff.each",
        "808080800140a11540128080000222",
        "808080800140a115401280800122",
    ),
    (
        "call.p2.diff.none",
        "808080800140a11140120080000222",
        "808080800140a111401200800122",
    ),
    (
        "write.p2.diff.none",
        "808080800140a11540120080000222",
        "808080800140a115401200800122",
    ),
];

/// EVO-81 skew: the multi request bytes of before and after the SW-81 writer fix are read the same by every
/// reader, both ways. `MultiArgs` is not `final_structure`, so its body is size-delimited and the old padding
/// byte sits inside it, where nothing reads it. The readers are unchanged by the fix (cf822c4e9 changes only
/// `RefVec::ser_shrink_wrap_vec_u8`), so a device or host built before it reads both forms as this test does.
#[test]
fn multi_empty_args_old_and_new_bytes_read_the_same() {
    use wire_weaver::prelude::*;
    use ww_client_server::{Request, RequestOwned};

    for (name, old_hex, new_hex) in MULTI_EMPTY_ARGS {
        let old = hex::decode(old_hex).expect("hex");
        let new = hex::decode(new_hex).expect("hex");
        assert_eq!(old.len(), new.len() + 1, "{name}: one padding byte less");

        let old_value =
            Request::from_ww_bytes(&old).unwrap_or_else(|e| panic!("{name} old: {e:?}"));
        let new_value =
            Request::from_ww_bytes(&new).unwrap_or_else(|e| panic!("{name} new: {e:?}"));
        assert_eq!(
            format!("{old_value:?}"),
            format!("{new_value:?}"),
            "{name}: borrowed reader"
        );

        let old_owned = RequestOwned::from_ww_bytes_owned(&old)
            .unwrap_or_else(|e| panic!("{name} old owned: {e:?}"));
        let new_owned = RequestOwned::from_ww_bytes_owned(&new)
            .unwrap_or_else(|e| panic!("{name} new owned: {e:?}"));
        assert_eq!(
            format!("{old_owned:?}"),
            format!("{new_owned:?}"),
            "{name}: owned reader"
        );

        // a std host (owned writer) has always sent the new form; the borrowed writer now does too
        assert_eq!(
            old_owned.to_ww_bytes_owned().expect("owned write"),
            new,
            "{name}: owned writer"
        );
        let mut buf = [0u8; 64];
        assert_eq!(
            old_value.to_ww_bytes(&mut buf).expect("borrowed write"),
            &new[..],
            "{name}: borrowed writer"
        );
    }
}

/// EVO-81 skew on a device: the golden multi_call request of before the fix gets the same reply and events from
/// the sync and the async server as the new one.
#[test]
fn multi_call_old_request_gets_the_new_reply() {
    let (_, old_hex, new_hex) = MULTI_EMPTY_ARGS[0];
    let corpus = corpus::load();
    let case = corpus
        .cases
        .iter()
        .find(|c| c.name == "multi.multi_call")
        .expect("multi.multi_call in the corpus");
    assert_eq!(hex::encode(requests::build(&case.name, 0)), new_hex);
    let golden_reply = case.steps[0].reply_bytes();

    for request in [
        hex::decode(old_hex).expect("hex"),
        hex::decode(new_hex).expect("hex"),
    ] {
        let mut srv = server::Server::default();
        let mut scratch = [0u8; 512];
        let mut event_scratch = [0u8; 512];
        let mut out = wire_weaver::EventWriter::new(Recorder::default(), &mut event_scratch);
        let reply = srv
            .process_request_bytes(&request, &mut scratch, &mut out, ())
            .expect("sync server")
            .to_vec();
        assert_eq!(
            reply,
            golden_reply,
            "sync reply to {}",
            hex::encode(&request)
        );
        assert!(out.sink.0.is_empty(), "sync events");

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let (reply, events) = rt.block_on(async {
            let mut srv = async_server::AsyncServer::default();
            let mut scratch = [0u8; 512];
            let mut event_scratch = [0u8; 512];
            let mut out = wire_weaver::EventWriter::new(Recorder::default(), &mut event_scratch);
            let reply = srv
                .process_request_bytes(&request, &mut scratch, &mut out, ())
                .await
                .expect("async server")
                .to_vec();
            (reply, out.sink.0)
        });
        assert_eq!(
            reply,
            golden_reply,
            "async reply to {}",
            hex::encode(&request)
        );
        assert!(events.is_empty(), "async events");
    }
}

/// EVO-81: events and replies are untouched by the fix. The only `RefVec<u8>` in an `Event` is
/// `ErrorKind::UserBytes`, which follows a `u8` discriminant and so always starts on a byte: an empty one (a
/// property error without a user error type) has the bytes the pre-fix commit wrote, for each length of `seq`
/// and `err_seq`.
#[test]
fn empty_user_bytes_event_unchanged() {
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::UVlq32;
    use ww_client_server::{Error, ErrorKind, Event, EventOwned};

    const PRE_FIX: &[(u32, u32, &str)] = &[
        (0, 0, "00000c0002"),
        (0, 1, "01000c0002"),
        (0, 300, "822c000c0002"),
        (3, 1, "01030c0002"),
        (300, 1, "010cd40c0002"),
        (70000, 300, "822c08ed89200c0002"),
    ];
    for (err_seq, seq, pre_fix) in PRE_FIX {
        let ev = Event {
            seq: UVlq32(*seq),
            result: Err(Error::new(
                *err_seq,
                ErrorKind::UserBytes(RefVec::Slice { slice: &[] }),
            )),
        };
        let mut buf = [0u8; 64];
        let bytes = ev.to_ww_bytes(&mut buf).expect("borrowed write");
        assert_eq!(hex::encode(bytes), *pre_fix, "err_seq {err_seq} seq {seq}");
        let owned = EventOwned::from_ww_bytes_owned(bytes).expect("owned read");
        assert_eq!(
            hex::encode(owned.to_ww_bytes_owned().expect("owned write")),
            *pre_fix
        );
    }
}

/// A server without a `receiver_model` serves a host runtime (SRV-12) the same way as one with: `is_shared` says no
/// to every request, and the shared entry refuses whatever reaches it at `REQUEST_NOT_SHARED`.
#[test]
fn without_a_receiver_model_nothing_is_shared() {
    use wire_weaver::prelude::*;
    use ww_client_server::{ErrorKindOwned, EventOwned, err_seq};
    let srv = server::Server::default();
    let origin = wire_weaver::CallContext::new(0);
    for case in &corpus::load().cases {
        for i in 0..case.steps.len() {
            let request = requests::build(&case.name, i);
            assert!(
                !server::Server::is_shared(&request),
                "case '{}' step {i}",
                case.name
            );
            let mut scratch = [0u8; 512];
            let mut event_scratch = [0u8; 512];
            let mut out = wire_weaver::EventWriter::new(Recorder::default(), &mut event_scratch);
            let reply = srv
                .process_shared_from(&origin, &request, &mut scratch, &mut out, ())
                .expect("the refusal is written")
                .to_vec();
            assert!(
                out.sink.0.is_empty(),
                "case '{}' step {i}: no handler ran",
                case.name
            );
            if ww_client_server::Request::peek_seq(&request).expect("a request") == 0 {
                assert!(reply.is_empty(), "case '{}' step {i}", case.name);
                continue;
            }
            let event = EventOwned::from_ww_bytes_owned(&reply).expect("an event");
            let error = event.result.expect_err("refused");
            assert_eq!(error.err_seq(), err_seq::REQUEST_NOT_SHARED);
            assert!(
                matches!(error.kind, ErrorKindOwned::OperationNotSupported),
                "{error:?}"
            );
        }
    }
}
