//! The `pattern=keyword` syntax of the four models (`crate::pattern_model`), checked the same way for each: inputs written with `X` and `Y`
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

/// `expected` is `Ok(summary)` or `Err(part of the message)`; an error names the model.
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
                got.starts_with(&format!("ERR {model}")) && got.contains(&part),
                "{model} {written:?}: {got}, expected an error with {part:?}"
            );
        }
    }
}

#[rustfmt::skip]
const CASES: &[(&str, Result<&str, &str>)] = &[
    // accepted
    ("", Ok("_=-")),
    (" ", Ok("_=-")),
    ("_=X", Ok("_=X")),
    ("_=X,", Ok("_=X")),
    ("_=X ,", Ok("_=X")),
    ("a=X, _=Y", Ok("_=Y a=X")),
    ("_=Y, a=X", Ok("_=Y a=X")),
    ("a=X,_=Y", Ok("_=Y a=X")),
    ("a = X", Ok("_=- a=X")),
    (".*move|b.c=X, d=Y", Ok("_=- .*move|b.c=X d=Y")),
    ("a{1,2}=X", Ok("_=- a{1,2}=X")),
    ("a,b=X", Ok("_=- a,b=X")),
    ("  a = X , _ = Y  ", Ok("_=Y a=X")),
    ("\ta=X", Ok("_=- a=X")),
    ("a=X,\n_=Y", Ok("_=Y a=X")),
    ("a=X,\n  _=Y,\n", Ok("_=Y a=X")),
    // rejected
    (",", Err("at 1:1: expected a `pattern=keyword` item, found `,`")),
    ("a=X,,b=Y", Err("expected a `pattern=keyword` item, found `,`")),
    ("=X", Err("at 1:1: expected a pattern before `=`")),
    ("a", Err("at 1:2: expected `=` after `a`, found the end")),
    ("a b=X", Err("at 1:3: expected `=` after `a`, found `b`; a pattern holds no whitespace")),
    ("a=X,\n  b c=Y", Err("at 2:5: expected `=` after `b`, found `c`")),
    ("a=", Err("at 1:3: expected ")),
    ("a=", Err(" after `a=`, found the end")),
    ("a==X", Err(" after `a=`, found `=`")),
    ("a=nope", Err("at 1:3: unknown `nope` in `a=nope`, expected `")),
    ("a=Xz", Err("unknown `Xz` in `a=Xz`")),
    ("a=X_=Y", Err("unknown `X_` in `a=X_`")),
    ("a=X _=Y", Err("expected `,` after `a=X`, found `_`")),
    ("a=X y", Err("expected `,` after `a=X`, found `y`")),
    ("a=X garbage", Err("expected `,` after `a=X`, found `g`")),
    ("a=X;b=Y", Err("expected `,` after `a=X`, found `;`")),
    ("a=X+Y", Err("expected `,` after `a=X`, found `+`")),
    ("a=X + Y", Err("expected `,` after `a=X`, found `+`")),
    ("_=X, _=Y", Err("multiple defaults (`_=..`)")),
    ("a(=X", Err("invalid pattern `a(`: regex parse error")),
    ("a(=X", Err("unclosed group")),
];

#[test]
fn syntax() {
    for &(input, expected) in CASES {
        check(
            "method_model",
            method,
            "immediate",
            "deferred",
            input,
            expected,
        );
        check(
            "property_model",
            property,
            "get_set",
            "value_on_changed",
            input,
            expected,
        );
        check("const_model", konst, "fn", "const", input, expected);
        check("handler_model", handler, "async", "sync", input, expected);
    }
}
