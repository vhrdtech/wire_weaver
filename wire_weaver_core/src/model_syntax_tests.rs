//! The `pattern=keyword` syntax of the four models, pinned the same way for each: inputs written with `X` and `Y`
//! for the model's two keywords, outcomes as `_=K pattern=K ..` (the default first, `-` when none) or `ERR text`.
use crate::const_model::{ConstModel, ConstModelKind};
use crate::handler_model::{HandlerKind, HandlerModel};
use crate::method_model::{MethodModel, MethodModelKind};
use crate::property_model::{PropertyModel, PropertyModelKind};

fn summary(default: Option<&str>, items: Vec<(String, &str)>) -> String {
    let mut out = format!("_={}", default.unwrap_or("-"));
    for (pattern, kind) in items {
        out.push_str(&format!(" {pattern}={kind}"));
    }
    out
}

fn anchored(regex: &str) -> String {
    let pattern = regex
        .strip_prefix("^(?:")
        .and_then(|r| r.strip_suffix(")$"));
    pattern.expect("an anchored pattern").to_owned()
}

fn method(input: &str) -> String {
    let kind = |k: MethodModelKind| {
        if k == MethodModelKind::Immediate {
            "X"
        } else {
            "Y"
        }
    };
    match MethodModel::parse(input) {
        Ok(m) => summary(
            m.default.map(kind),
            m.items
                .iter()
                .map(|i| (i.regex.as_str().to_owned(), kind(i.model)))
                .collect(),
        ),
        Err(e) => format!("ERR {e}"),
    }
}

fn property(input: &str) -> String {
    let kind = |k: PropertyModelKind| match k {
        PropertyModelKind::GetSet => "X",
        PropertyModelKind::ValueOnChanged => "Y",
    };
    match PropertyModel::parse(input) {
        Ok(m) => summary(
            m.default.map(kind),
            m.items
                .iter()
                .map(|i| (i.regex.as_str().to_owned(), kind(i.model)))
                .collect(),
        ),
        Err(e) => format!("ERR {e}"),
    }
}

fn konst(input: &str) -> String {
    let kind = |k: ConstModelKind| if k == ConstModelKind::Fn { "X" } else { "Y" };
    match ConstModel::parse(input) {
        Ok(m) => summary(
            m.default.map(kind),
            m.items
                .iter()
                .map(|i| (anchored(i.regex.as_str()), kind(i.model)))
                .collect(),
        ),
        Err(e) => format!("ERR {e}"),
    }
}

fn handler(input: &str) -> String {
    let kind = |k: HandlerKind| if k == HandlerKind::Async { "X" } else { "Y" };
    match HandlerModel::parse(input) {
        Ok(m) => summary(
            m.default.map(kind),
            m.items
                .iter()
                .map(|i| {
                    assert_eq!(
                        anchored(i.regex.as_str()),
                        i.pattern,
                        "the regex is the pattern anchored"
                    );
                    (i.pattern.clone(), kind(i.kind))
                })
                .collect(),
        ),
        Err(e) => format!("ERR {e}"),
    }
}

/// `expected` is `Ok(summary)` or `Err(part of the message)`.
fn check(
    model: &str,
    parse: fn(&str) -> String,
    x: &str,
    y: &str,
    input: &str,
    expected: Result<&str, &str>,
) {
    let written = input.replace('X', x).replace('Y', y);
    let got = parse(&written);
    match expected {
        Ok(summary) => assert_eq!(got, summary, "{model} {written:?}"),
        Err(part) => {
            let part = part.replace('X', x).replace('Y', y);
            assert!(
                got.starts_with("ERR ") && got.contains(&part),
                "{model} {written:?}: {got}, expected an error with {part:?}"
            );
        }
    }
}

/// One input and its outcome in `method_model` and `property_model`, in `const_model`, and in the pattern models
/// (`handler_model`, `receiver_model`).
type Case = (
    &'static str,
    Result<&'static str, &'static str>,
    Result<&'static str, &'static str>,
    Result<&'static str, &'static str>,
);

#[rustfmt::skip]
const CASES: &[Case] = &[
    // accepted
    ("", Ok("_=-"), Ok("_=-"), Ok("_=-")),
    (" ", Ok("_=-"), Ok("_=-"), Ok("_=-")),
    ("_=X", Ok("_=X"), Ok("_=X"), Ok("_=X")),
    ("_=X,", Ok("_=X"), Ok("_=X"), Ok("_=X")),
    ("_=X ,", Ok("_=X"), Ok("_=X"), Ok("_=X")),
    ("a=X, _=Y", Ok("_=Y a=X"), Ok("_=Y a=X"), Ok("_=Y a=X")),
    ("_=Y, a=X", Ok("_=Y a=X"), Ok("_=Y a=X"), Ok("_=Y a=X")),
    ("a=X,_=Y", Ok("_=Y a=X"), Ok("_=Y a=X"), Ok("_=Y a=X")),
    ("a = X", Ok("_=- a=X"), Ok("_=- a=X"), Ok("_=- a=X")),
    (".*move|b.c=X, d=Y", Ok("_=- .*move|b.c=X d=Y"), Ok("_=- .*move|b.c=X d=Y"), Ok("_=- .*move|b.c=X d=Y")),
    ("a{1,2}=X", Ok("_=- a{1,2}=X"), Ok("_=- a{1,2}=X"), Err("expected EOI or item")),
    // whitespace: method and property models have no leading whitespace and no newlines
    ("  a = X , _ = Y  ", Ok("_=-"), Ok("_=Y a=X"), Ok("_=Y a=X")),
    ("\ta=X", Ok("_=-"), Ok("_=- a=X"), Ok("_=- a=X")),
    ("a=X,\n_=Y", Ok("_=- a=X \n_=Y"), Ok("_=- a=X \n_=Y"), Ok("_=Y a=X")),
    ("a=X,\n  _=Y,\n", Ok("_=- a=X \n  _=Y"), Err("expected EOI or item"), Ok("_=Y a=X")),
    // quirks: no `,` needed between items, whitespace and `,` inside patterns
    ("a=X _=Y", Ok("_=Y a=X"), Ok("_=Y a=X"), Err("expected EOI or regex")),
    ("a=X_=Y", Ok("_=Y a=X"), Ok("_=Y a=X"), Err("expected EOI or regex")),
    ("a b=X", Ok("_=- a b=X"), Ok("_=- a b=X"), Ok("_=- a b=X")),
    ("a,b=X", Ok("_=- a,b=X"), Ok("_=- a,b=X"), Err("expected EOI or item")),
    ("a=X,,b=Y", Ok("_=- a=X ,b=Y"), Ok("_=- a=X ,b=Y"), Err("expected EOI or regex")),
    ("a=X;b=Y", Ok("_=- a=X ;b=Y"), Ok("_=- a=X ;b=Y"), Ok("_=- a=X ;b=Y")),
    // method and property models stop at the first thing they don't understand and keep what came before
    (",", Ok("_=-"), Err("expected EOI or item"), Err("expected EOI or regex")),
    ("a=Xz", Ok("_=- a=X"), Err("expected EOI or item"), Err("unknown `Xz` in `a=Xz`")),
    ("a=X y", Ok("_=- a=X"), Err("expected EOI or item"), Err("unknown `X y` in `a=X y`")),
    ("a=X garbage", Ok("_=- a=X"), Err("expected EOI or item"), Err("unknown `X garbage`")),
    ("a=X+Y", Ok("_=- a=X"), Err("expected EOI or item"), Err("expected EOI or item")),
    ("a=X + Y", Ok("_=- a=X"), Err("expected EOI or item"), Err("expected EOI or item")),
    ("a=", Ok("_=-"), Err("expected model"), Err("expected model")),
    ("=X", Ok("_=-"), Err("expected EOI or regex"), Err("expected EOI or regex")),
    ("a", Ok("_=-"), Err("expected EOI or item"), Err("expected EOI or item")),
    ("a=nope", Ok("_=-"), Err("expected model"), Err("unknown `nope` in `a=nope`")),
    ("a==X", Ok("_=-"), Err("expected model"), Err("expected model")),
    // rejected everywhere
    ("_=X, _=Y", Err("Multiple default values"), Err("Multiple default values"), Err("multiple defaults (`_=..`)")),
    ("a(=X", Err("unclosed group"), Err("unclosed group"), Err("unclosed group")),
];

#[test]
fn syntax() {
    for &(input, method_property, const_, pattern) in CASES {
        check(
            "method_model",
            method,
            "immediate",
            "deferred",
            input,
            method_property,
        );
        check(
            "property_model",
            property,
            "get_set",
            "value_on_changed",
            input,
            method_property,
        );
        check("const_model", konst, "fn", "const", input, const_);
        check("handler_model", handler, "async", "sync", input, pattern);
    }
}
