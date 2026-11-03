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
