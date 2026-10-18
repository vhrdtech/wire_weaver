//! `corpus.json`: the cases, read the same way by every runner (this crate and `wire_weaver_py`'s
//! `tests/test_conformance.py`).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The consumers, in report order.
pub const CONSUMERS: [&str; 5] = ["derive", "dynamic", "explain", "python", "c"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    /// What the file is, for a person opening it.
    #[serde(rename = "_about")]
    pub about: Vec<String>,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Unique, `area.what`: `int.u8.max`, `enum.unknown_variant`.
    pub name: String,
    /// The property of `conformance_api::Conformance` with the type of this case (`u8`, `shape`), or the Rust type
    /// name of one that isn't in the API model (`HeadTail`).
    #[serde(rename = "type")]
    pub ty: String,
    /// The value, in the form `wire_weaver_py` uses (`docs/serdes/conformance.md`). `null` for `reject` cases.
    #[serde(default)]
    pub value: serde_json::Value,
    /// The golden bytes as hex (spaces allowed), none for `reject_value` cases, `?` until `CONFORMANCE=update` fills
    /// them in.
    #[serde(default)]
    pub bytes: String,
    /// The golden bytes are those of another case (`CONFORMANCE=update` copies them): old data for an evolved type,
    /// a newer variant for an older reader.
    #[serde(default)]
    pub bytes_from: Option<String>,
    #[serde(default)]
    pub direction: Direction,
    /// Consumers that can't express this case yet: consumer -> why, with the FEATURES.md id that will fix it.
    #[serde(default)]
    pub gaps: BTreeMap<String, String>,
    /// What the case checks, when the name doesn't say it all.
    #[serde(default)]
    pub note: Option<String>,
    /// A deliberate wire break (TEST-24, `docs/serdes/conformance.md`): `"EVO-n / reason"`. Without it, the frozen
    /// bytes guard fails when this case's `bytes`, `direction` or `bytes_from` differ from what they were at the
    /// merge base with main; with it, the change is accepted, but still needs the matching EVO item and a
    /// `**Breaking:**` entry in the crate's CHANGELOG.
    #[serde(default)]
    pub wire_change: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// `value` encodes to `bytes` and `bytes` decode to `value`.
    #[default]
    Both,
    /// `bytes` decode to `value`, but `value` encodes to something else: old data read by a newer type, a variant
    /// from a newer version read as unknown.
    Decode,
    /// `bytes` don't decode (malformed, a variant compiled out); `value` is `null`.
    Reject,
    /// `value` doesn't encode (over a `MaxLen` bound); `bytes` is empty.
    RejectValue,
}

/// Golden bytes still to come, filled in by `CONFORMANCE=update`.
const TO_FILL: &str = "\"bytes\": \"?\"";

impl Case {
    /// The golden bytes are still to come (`"bytes": "?"`).
    pub fn to_fill(&self) -> bool {
        self.bytes == "?"
    }

    pub fn golden(&self) -> Vec<u8> {
        assert!(
            !self.to_fill(),
            "case '{}' has no golden bytes yet: just conformance-update",
            self.name
        );
        let hex: String = self.bytes.split_whitespace().collect();
        hex::decode(&hex).unwrap_or_else(|e| panic!("{}: bytes are not hex: {e}", self.name))
    }

    pub fn gap(&self, consumer: &str) -> Option<&str> {
        self.gaps.get(consumer).map(String::as_str)
    }
}

pub fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus.json")
}

pub fn load() -> Corpus {
    let text = std::fs::read_to_string(path()).expect("tests/conformance/corpus.json");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("corpus.json: {e}"))
}

/// Index of the first differing byte, or the shorter length.
pub fn first_diff(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

/// `case 'x', consumer: ...` with the first differing byte.
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

/// `CONFORMANCE=update`: the golden bytes of every case that has them still to come (`"bytes": "?"`), from the
/// derive or from the case named in `bytes_from`. The file is edited in place, everything else is kept as written.
pub fn fill_in_bytes(derive: &crate::derive::Derive) {
    let path = path();
    let mut text = std::fs::read_to_string(&path).expect("corpus.json");
    let corpus = load();
    let encoded = |name: &str| -> Vec<u8> {
        let typed = derive
            .get(name)
            .unwrap_or_else(|| panic!("case '{name}' is not in derive.rs"));
        (typed.encode)()
            .unwrap_or_else(|e| panic!("case '{name}': the derive can't encode it: {e}"))
    };
    let mut fills = vec![];
    for case in &corpus.cases {
        if !case.to_fill() {
            continue;
        }
        let bytes = match &case.bytes_from {
            Some(from) => {
                let other = corpus.cases.iter().find(|c| &c.name == from);
                match other {
                    Some(o) if !o.to_fill() => o.golden(),
                    Some(_) => encoded(from),
                    None => panic!("case '{}': bytes_from '{from}' is no case", case.name),
                }
            }
            None => encoded(&case.name),
        };
        fills.push(hex::encode(bytes));
    }
    for hex in fills {
        let at = text
            .find(TO_FILL)
            .expect("one placeholder per case to fill");
        text.replace_range(at..at + TO_FILL.len(), &format!("\"bytes\": \"{hex}\""));
    }
    std::fs::write(&path, text).expect("corpus.json");
}
