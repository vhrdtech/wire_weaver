//! Shape cases (TEST-37): every list-like and string-like field kind, empty, with one element and with many, at a
//! byte-aligned start and at a mid-byte start (after a `bool`), last in its struct and followed by a `bool`. These
//! are the places where writers have disagreed (SW-81: the borrowed derive padded an empty byte list that started
//! mid-byte), so each one is a case that every consumer runs, the borrowed type both as read from the golden bytes
//! and as built from slices.
//!
//! This table is the only source: [render_api] writes the types and properties into `conformance_api` (between the
//! `shape` markers), [render_values] the Rust values (`shape_values.rs`), [add_missing_cases] the corpus cases,
//! which [check_corpus] compares. The `shapes` test fails when one of them is not what the table gives;
//! `just conformance-update` writes them.
//! The coverage guard (TEST-25, [crate::coverage]) fails when a field kind of the API model has no shape cases.

use crate::corpus::Corpus;
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// A value of a field kind.
pub struct Value {
    /// The count's name in the case name: `empty`, `one` or `many`.
    pub count: &'static str,
    /// The field's type (borrowed form, lifetime `'i`); an array's length is part of its type.
    pub field: &'static str,
    /// In the corpus's form.
    pub json: &'static str,
    /// As a Rust expression of the owned type.
    pub owned: &'static str,
    /// As a Rust expression of the borrowed type, built from slices.
    pub borrowed: &'static str,
}

/// A field kind and its three values.
pub struct Kind {
    /// Its name in case names, and what [crate::coverage] calls it.
    pub name: &'static str,
    /// What its owned and borrowed types are, for the doc comment.
    pub doc: &'static str,
    /// The field type has a lifetime, so the derive makes a separate owned type.
    pub borrows: bool,
    /// Only as the last field (`TailBytes`).
    pub last_only: bool,
    /// Gap marks of its cases besides C's, as `(consumer, reason)`.
    pub gaps: &'static [(&'static str, &'static str)],
    pub values: [Value; 3],
}

impl Kind {
    /// In the API model: a property of `Conformance` per type. Kinds with gaps of the dynamic consumers aren't.
    fn in_model(&self) -> bool {
        !self.gaps.iter().any(|(c, _)| *c == "dynamic")
    }
}

/// Where the field sits: after a `bool` or not, followed by a `bool` or not.
pub struct Layout {
    pub name: &'static str,
    pub camel: &'static str,
    pub mid: bool,
    pub then_bool: bool,
}

pub const LAYOUTS: [Layout; 4] = [
    Layout {
        name: "aligned_last",
        camel: "AlignedLast",
        mid: false,
        then_bool: false,
    },
    Layout {
        name: "aligned_bool",
        camel: "AlignedBool",
        mid: false,
        then_bool: true,
    },
    Layout {
        name: "mid_last",
        camel: "MidLast",
        mid: true,
        then_bool: false,
    },
    Layout {
        name: "mid_bool",
        camel: "MidBool",
        mid: true,
        then_bool: true,
    },
];

const C_GAP: &str = "no generated C for this type yet: C from the ww_self model (API-26)";
const TAIL_GAP: &str = "TailBytes has no form in the API model yet (INT-9)";

const fn v(
    count: &'static str,
    field: &'static str,
    json: &'static str,
    owned: &'static str,
    borrowed: &'static str,
) -> Value {
    Value {
        count,
        field,
        json,
        owned,
        borrowed,
    }
}

pub const KINDS: &[Kind] = &[
    Kind {
        name: "bytes",
        doc: "a byte list, `RefVec<'i, u8>` / `Vec<u8>`",
        borrows: true,
        last_only: false,
        gaps: &[],
        values: [
            v(
                "empty",
                "RefVec<'i, u8>",
                "[]",
                "vec![]",
                "RefVec::Slice { slice: &[] }",
            ),
            v(
                "one",
                "RefVec<'i, u8>",
                "[7]",
                "vec![7]",
                "RefVec::Slice { slice: &[7] }",
            ),
            v(
                "many",
                "RefVec<'i, u8>",
                "[1, 2, 3]",
                "vec![1, 2, 3]",
                "RefVec::Slice { slice: &[1, 2, 3] }",
            ),
        ],
    },
    Kind {
        name: "vec",
        doc: "a list of multi-byte numbers, `RefVec<'i, u16>` / `Vec<u16>`",
        borrows: true,
        last_only: false,
        gaps: &[],
        values: [
            v(
                "empty",
                "RefVec<'i, u16>",
                "[]",
                "vec![]",
                "RefVec::Slice { slice: &[] }",
            ),
            v(
                "one",
                "RefVec<'i, u16>",
                "[4660]",
                "vec![0x1234]",
                "RefVec::Slice { slice: &[0x1234] }",
            ),
            v(
                "many",
                "RefVec<'i, u16>",
                "[1, 4660, 65535]",
                "vec![1, 0x1234, 0xFFFF]",
                "RefVec::Slice { slice: &[1, 0x1234, 0xFFFF] }",
            ),
        ],
    },
    Kind {
        name: "vec_bool",
        doc: "a list of bits, `RefVec<'i, bool>` / `Vec<bool>`",
        borrows: true,
        last_only: false,
        gaps: &[],
        values: [
            v(
                "empty",
                "RefVec<'i, bool>",
                "[]",
                "vec![]",
                "RefVec::Slice { slice: &[] }",
            ),
            v(
                "one",
                "RefVec<'i, bool>",
                "[true]",
                "vec![true]",
                "RefVec::Slice { slice: &[true] }",
            ),
            v(
                "many",
                "RefVec<'i, bool>",
                "[true, false, true]",
                "vec![true, false, true]",
                "RefVec::Slice { slice: &[true, false, true] }",
            ),
        ],
    },
    Kind {
        name: "vec_str",
        doc: "a list of strings, `RefVec<'i, &'i str>` / `Vec<String>`",
        borrows: true,
        last_only: false,
        gaps: &[],
        values: [
            v(
                "empty",
                "RefVec<'i, &'i str>",
                "[]",
                "vec![]",
                "RefVec::Slice { slice: &[] }",
            ),
            v(
                "one",
                "RefVec<'i, &'i str>",
                "[\"a\"]",
                "vec![\"a\".to_owned()]",
                "RefVec::Slice { slice: &[\"a\"] }",
            ),
            v(
                "many",
                "RefVec<'i, &'i str>",
                "[\"a\", \"\", \"bc\"]",
                "vec![\"a\".to_owned(), String::new(), \"bc\".to_owned()]",
                "RefVec::Slice { slice: &[\"a\", \"\", \"bc\"] }",
            ),
        ],
    },
    Kind {
        name: "str",
        doc: "a string, `&'i str` / `String`",
        borrows: true,
        last_only: false,
        gaps: &[],
        values: [
            v("empty", "&'i str", "\"\"", "String::new()", "\"\""),
            v("one", "&'i str", "\"a\"", "\"a\".to_owned()", "\"a\""),
            v(
                "many",
                "&'i str",
                "\"abc\"",
                "\"abc\".to_owned()",
                "\"abc\"",
            ),
        ],
    },
    Kind {
        name: "option",
        doc: "an optional byte list, `Option<RefVec<'i, u8>>`: empty is `None`, one `Some` of an empty list, many \
            `Some` of three bytes",
        borrows: true,
        last_only: false,
        gaps: &[],
        values: [
            v("empty", "Option<RefVec<'i, u8>>", "null", "None", "None"),
            v(
                "one",
                "Option<RefVec<'i, u8>>",
                "[]",
                "Some(vec![])",
                "Some(RefVec::Slice { slice: &[] })",
            ),
            v(
                "many",
                "Option<RefVec<'i, u8>>",
                "[1, 2, 3]",
                "Some(vec![1, 2, 3])",
                "Some(RefVec::Slice { slice: &[1, 2, 3] })",
            ),
        ],
    },
    Kind {
        name: "array",
        doc: "a byte array, `[u8; N]`, of 0, 1 and 3 bytes",
        borrows: false,
        last_only: false,
        gaps: &[],
        values: [
            v("empty", "[u8; 0]", "[]", "[]", "[]"),
            v("one", "[u8; 1]", "[7]", "[7]", "[7]"),
            v("many", "[u8; 3]", "[1, 2, 3]", "[1, 2, 3]", "[1, 2, 3]"),
        ],
    },
    Kind {
        name: "tail",
        doc: "the rest of the buffer, `TailBytes<'i>` / `TailBytesOwned`, last only",
        borrows: true,
        last_only: true,
        gaps: &[
            ("dynamic", TAIL_GAP),
            ("explain", TAIL_GAP),
            ("python", TAIL_GAP),
        ],
        values: [
            v(
                "empty",
                "TailBytes<'i>",
                "[]",
                "TailBytesOwned(vec![])",
                "TailBytes(&[])",
            ),
            v(
                "one",
                "TailBytes<'i>",
                "[7]",
                "TailBytesOwned(vec![7])",
                "TailBytes(&[7])",
            ),
            v(
                "many",
                "TailBytes<'i>",
                "[1, 2, 3]",
                "TailBytesOwned(vec![1, 2, 3])",
                "TailBytes(&[1, 2, 3])",
            ),
        ],
    },
];

fn camel(snake: &str) -> String {
    snake
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// One type of the table: a kind in a layout (and, for an array, a length).
pub struct ShapeType<'a> {
    pub kind: &'a Kind,
    pub layout: &'a Layout,
    /// The value it is the type of, when its length is part of the type (an array), else `None`.
    pub value: Option<&'a Value>,
}

impl<'a> ShapeType<'a> {
    /// `FieldShapeBytesMidBool`, `FieldShapeArray3MidBool`.
    pub fn name(&self) -> String {
        let len = self.value.map(array_len).unwrap_or_default();
        format!(
            "FieldShape{}{len}{}",
            camel(self.kind.name),
            self.layout.camel
        )
    }

    /// The property of `Conformance` (`shape_bytes_mid_bool`), or the Rust type name for one outside the model:
    /// a case's `type`.
    pub fn case_type(&self) -> String {
        if !self.kind.in_model() {
            return self.name();
        }
        let len = self.value.map(array_len).unwrap_or_default();
        format!("shape_{}{len}_{}", self.kind.name, self.layout.name)
    }

    fn field(&self) -> &'a str {
        self.value.unwrap_or(&self.kind.values[0]).field
    }

    fn values(&self) -> Vec<&'a Value> {
        match self.value {
            Some(v) => vec![v],
            None => self.kind.values.iter().collect(),
        }
    }
}

/// `3` of `[u8; 3]`.
fn array_len(value: &Value) -> String {
    value
        .field
        .rsplit_once("; ")
        .map(|(_, n)| n.trim_end_matches(']').to_owned())
        .unwrap_or_default()
}

/// Every type of the table, in table order.
pub fn types() -> Vec<ShapeType<'static>> {
    let mut out = vec![];
    for kind in KINDS {
        for layout in &LAYOUTS {
            if kind.last_only && layout.then_bool {
                continue;
            }
            if kind.borrows {
                out.push(ShapeType {
                    kind,
                    layout,
                    value: None,
                });
            } else {
                out.extend(kind.values.iter().map(|value| ShapeType {
                    kind,
                    layout,
                    value: Some(value),
                }));
            }
        }
    }
    out
}

/// A case of the table: its name, `shape.<kind>.<layout>.<count>`, its type and value.
pub struct ShapeCase<'a> {
    pub name: String,
    pub ty: ShapeType<'a>,
    pub value: &'a Value,
}

pub fn cases() -> Vec<ShapeCase<'static>> {
    let mut out = vec![];
    for ty in types() {
        for value in ty.values() {
            out.push(ShapeCase {
                name: case_name(ty.kind.name, ty.layout.name, value.count),
                ty: ShapeType {
                    kind: ty.kind,
                    layout: ty.layout,
                    value: ty.value,
                },
                value,
            });
        }
    }
    out
}

pub fn case_name(kind: &str, layout: &str, count: &str) -> String {
    format!("shape.{kind}.{layout}.{count}")
}

/// The layouts a kind has cases in.
pub fn layouts_of(kind: &Kind) -> impl Iterator<Item = &'static Layout> + '_ {
    LAYOUTS
        .iter()
        .filter(move |l| !(kind.last_only && l.then_bool))
}

const API_BEGIN_PROPS: &str = "    // BEGIN shape properties (TEST-37): generated by `just conformance-update` from tests/conformance/src/shapes.rs\n";
const API_END_PROPS: &str = "    // END shape properties\n";
const API_BEGIN_TYPES: &str = "// BEGIN shape types (TEST-37): generated by `just conformance-update` from tests/conformance/src/shapes.rs\n";
const API_END_TYPES: &str = "// END shape types\n";

pub fn api_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance_api/src/lib.rs")
}

pub fn values_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shape_values.rs")
}

fn replace_between(text: &str, begin: &str, end: &str, with: &str) -> String {
    let start = text.find(begin).expect("begin marker") + begin.len();
    let stop = start + text[start..].find(end).expect("end marker");
    format!("{}{with}{}", &text[..start], &text[stop..])
}

/// `conformance_api`'s source with the shape properties and types the table gives.
pub fn render_api(current: &str) -> String {
    let mut props = String::new();
    let mut tys = String::new();
    for ty in types() {
        let name = ty.name();
        if ty.kind.in_model() {
            let lt = if ty.kind.borrows { "<'i>" } else { "" };
            let _ = writeln!(props, "    property!(rw {}: {name}{lt});", ty.case_type());
        }
        let start = if ty.layout.mid {
            "after a bool"
        } else {
            "at a byte"
        };
        let end = if ty.layout.then_bool {
            "followed by a bool"
        } else {
            "last"
        };
        let _ = writeln!(
            tys,
            "\n/// Shape cases (TEST-37): {}, {start}, {end}.",
            ty.kind.doc
        );
        let (attrs, lt) = match (ty.kind.borrows, ty.kind.last_only) {
            (true, true) => ("owned, derive(Debug)", "<'i>"),
            (true, false) => ("owned, derive(Debug, PartialEq, Clone)", "<'i>"),
            (false, _) => ("borrowed, owned, derive(Debug, PartialEq, Clone)", ""),
        };
        let _ = writeln!(
            tys,
            "#[derive_shrink_wrap({attrs})]\npub struct {name}{lt} {{"
        );
        if ty.layout.mid {
            tys.push_str("    pub a: bool,\n");
        }
        let _ = writeln!(tys, "    pub x: {},", ty.field());
        if ty.layout.then_bool {
            tys.push_str("    pub z: bool,\n");
        }
        tys.push_str("}\n");
    }
    let text = replace_between(current, API_BEGIN_PROPS, API_END_PROPS, &props);
    replace_between(&text, API_BEGIN_TYPES, API_END_TYPES, &tys)
}

/// The struct literal of a case, `{ a: true, x: <value>, z: true }`.
fn literal(case: &ShapeCase, value: &str) -> String {
    let mut fields = vec![];
    if case.ty.layout.mid {
        fields.push("a: true".to_owned());
    }
    fields.push(format!("x: {value}"));
    if case.ty.layout.then_bool {
        fields.push("z: true".to_owned());
    }
    format!("{} {{ {} }}", case.ty.name(), fields.join(", "))
}

/// `shape_values.rs` as the table gives it, before rustfmt.
pub fn render_values() -> String {
    let mut out = String::from(
        "//! Generated by `just conformance-update` from `shapes.rs` (TEST-37), do not edit.\n\n\
         use crate::borrowed::{RoundTrip, SliceCase};\n\
         use crate::derive::{Typed, case, case_debug};\n\
         use conformance_api::*;\n\
         use wire_weaver::prelude::*;\n\
         use wire_weaver::shrink_wrap::tail_bytes::{TailBytes, TailBytesOwned};\n\n\
         /// The derive's value of every shape case.\n\
         pub fn owned() -> Vec<Typed> {\n    vec![\n",
    );
    for case in cases() {
        let lit = literal(&case, case.value.owned);
        let owned = if case.ty.kind.borrows {
            lit.replacen(&case.ty.name(), &format!("{}Owned", case.ty.name()), 1)
        } else {
            lit
        };
        if case.ty.kind.last_only {
            let _ = writeln!(out, "        case_debug({:?}, || {owned}),", case.name);
        } else {
            let _ = writeln!(out, "        case({:?}, {owned}),", case.name);
        }
    }
    out.push_str(
        "    ]\n}\n\n/// The borrowed value of every shape case, built from slices.\n\
         pub const BORROWED: &[(&str, SliceCase)] = &[\n",
    );
    for case in cases() {
        let lit = literal(&case, case.value.borrowed);
        let ty = case.ty.name();
        let _ = writeln!(
            out,
            "    ({:?}, SliceCase {{ write: |buf| {lit}.to_ww_bytes(buf).map(|b| b.to_vec()), read: |bytes| \
             {ty}::from_ww_bytes(bytes).map(|v| format!(\"{{v:?}}\") == format!(\"{{:?}}\", {lit})) }}),",
            case.name
        );
    }
    out.push_str(
        "];\n\n/// The borrowed type of every shape type, by its case type.\n\
         pub const TYPES: &[(&str, RoundTrip)] = crate::borrowed::round_trips![\n",
    );
    for ty in types() {
        let lt = if ty.kind.borrows { "<'_>" } else { "" };
        let _ = writeln!(out, "    {:?} => {}{lt},", ty.case_type(), ty.name());
    }
    out.push_str("];\n");
    out
}

/// `text` through rustfmt (edition 2024, the workspace's), so the generated file is what `cargo fmt` leaves.
pub fn rustfmt(text: &str) -> String {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("rustfmt on PATH");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(text.as_bytes())
        .expect("write to rustfmt");
    let out = child.wait_with_output().expect("rustfmt");
    assert!(out.status.success(), "rustfmt failed on shape_values.rs");
    String::from_utf8(out.stdout).expect("rustfmt output is UTF-8")
}

/// A case's value in the corpus's form, `{"a": true, "x": .., "z": true}`.
fn value_json(case: &ShapeCase) -> String {
    let mut fields = vec![];
    if case.ty.layout.mid {
        fields.push("\"a\": true".to_owned());
    }
    fields.push(format!("\"x\": {}", case.value.json));
    if case.ty.layout.then_bool {
        fields.push("\"z\": true".to_owned());
    }
    format!("{{{}}}", fields.join(", "))
}

/// A case of the table as it goes into `corpus.json`, golden bytes still to fill in.
fn case_json(case: &ShapeCase) -> String {
    format!(
        "    {{\n      \"name\": \"{}\",\n      \"type\": \"{}\",\n      \"value\": {},\n      \"bytes\": \"?\",\n      \
         \"gaps\": {},\n      \"note\": \"generated shape case (TEST-37)\"\n    }}",
        case.name,
        case.ty.case_type(),
        value_json(case),
        serde_json::to_string(&gaps(case.ty.kind)).expect("gaps as JSON"),
    )
}

fn gaps(kind: &Kind) -> serde_json::Map<String, serde_json::Value> {
    let mut gaps = serde_json::Map::new();
    for (consumer, reason) in kind.gaps {
        gaps.insert((*consumer).to_owned(), (*reason).into());
    }
    gaps.insert("c".into(), C_GAP.into());
    gaps
}

/// What is off between the table and the corpus: a case missing, or one whose type, value or gaps differ.
pub fn check_corpus(corpus: &Corpus) -> Vec<String> {
    let mut failures = vec![];
    for case in cases() {
        let Some(c) = corpus.cases.iter().find(|c| c.name == case.name) else {
            failures.push(format!("shape case '{}' is not in corpus.json", case.name));
            continue;
        };
        let value: serde_json::Value =
            serde_json::from_str(&value_json(&case)).expect("shape value as JSON");
        let want_gaps: std::collections::BTreeMap<String, String> = gaps(case.ty.kind)
            .into_iter()
            .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_owned()))
            .collect();
        if c.ty != case.ty.case_type() || c.value != value || c.gaps != want_gaps {
            failures.push(format!(
                "shape case '{}' in corpus.json is not what shapes.rs gives",
                case.name
            ));
        }
    }
    failures
}

/// `corpus.json` with the shape cases it lacks added at the end of `cases`, bytes to fill in.
pub fn add_missing_cases(text: &str, corpus: &Corpus) -> String {
    let missing: Vec<String> = cases()
        .iter()
        .filter(|case| !corpus.cases.iter().any(|c| c.name == case.name))
        .map(case_json)
        .collect();
    if missing.is_empty() {
        return text.to_owned();
    }
    let end = "\n  ],\n  \"requantize\": [";
    let at = text.find(end).expect("the end of cases in corpus.json");
    format!("{},\n{}{}", &text[..at], missing.join(",\n"), &text[at..])
}
