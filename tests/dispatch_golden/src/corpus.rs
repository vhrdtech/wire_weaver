//! `corpus.json`: one case per resource kind or outcome the dispatcher serves (TEST-31), each a sequence of
//! requests against a fresh server, with the direct reply and any events the handler queued while answering.
//! Read the same way every time, filled in by `just dispatch-golden-update`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    /// What the file is, for a person opening it.
    #[serde(rename = "_about")]
    pub about: Vec<String>,
    pub cases: Vec<Case>,
}

/// One or more requests against a fresh server: [crate::requests::build] gives the request bytes of each step by
/// the case's `name`, this struct holds what a step answered with.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Unique, `kind.what`: `call.with_args`, `error.bad_path`.
    pub name: String,
    pub steps: Vec<Step>,
    /// What the case checks, when the name doesn't say it all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A deliberate wire break (TEST-24-style escape, `docs/serdes/conformance.md`): `"EVO-n / reason"`, needs the
    /// matching `EVO-n` item and a `**Breaking:**` CHANGELOG entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_change: Option<String>,
}

/// One request of a [Case], against the server state its earlier steps left behind.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// The serialized `ww_client_server::Request`, hex, from [crate::requests::build]. `"?"` until
    /// `DISPATCH_GOLDEN=update` fills it in.
    pub request: String,
    /// The direct reply `process_request_bytes` returns for this request, hex, golden; empty for no reply (seq 0,
    /// a `Deferred` method, a plain `SinkWrite`). `"?"` until filled in.
    #[serde(default)]
    pub reply: String,
    /// Events the handler queued while answering this request (e.g., the deferred reply to an earlier request of
    /// the same case), hex, golden, in order; pre-sized with `"?"` placeholders by whoever adds the case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
}

/// Golden bytes still to come, filled in by `DISPATCH_GOLDEN=update`.
const TO_FILL: &str = "?";

impl Step {
    pub fn request_to_fill(&self) -> bool {
        self.request == TO_FILL
    }

    pub fn reply_to_fill(&self) -> bool {
        self.reply == TO_FILL
    }

    pub fn request_bytes(&self) -> Vec<u8> {
        decode(&self.request, "request")
    }

    pub fn reply_bytes(&self) -> Vec<u8> {
        decode(&self.reply, "reply")
    }

    pub fn event_bytes(&self) -> Vec<Vec<u8>> {
        self.events.iter().map(|e| decode(e, "event")).collect()
    }
}

fn decode(hex_str: &str, what: &str) -> Vec<u8> {
    assert_ne!(
        hex_str, TO_FILL,
        "{what} has no golden bytes yet: just dispatch-golden-update"
    );
    let hex: String = hex_str.split_whitespace().collect();
    hex::decode(&hex).unwrap_or_else(|e| panic!("{what} bytes are not hex: {e}"))
}

pub fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus.json")
}

pub fn load() -> Corpus {
    let text = std::fs::read_to_string(path()).expect("tests/dispatch_golden/corpus.json");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("corpus.json: {e}"))
}

/// Index of the first differing byte, or the shorter length.
pub fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

pub fn bytes_mismatch(got: &[u8], want: &[u8]) -> String {
    let at = first_diff(got, want);
    format!(
        "bytes differ at byte {at}: got {} ({} bytes), golden {} ({} bytes)",
        hex::encode(got),
        got.len(),
        hex::encode(want),
        want.len()
    )
}
