//! EVO-78: a `keyed!` companion is a resource of its own, added and removed as any other, whose key type works both
//! ways and which keys the same array on both versions, the array matched as any resource is.

use super::*;

/// A crate depending on this checkout's `ww_keyed`, as an API with `keyed!` must.
fn keyed_deps() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_stdlib/ww_keyed");
    format!(
        "ww_keyed = {{ path = {:?} }}\n",
        path.canonicalize().unwrap()
    )
}

/// `old` at crate 0.1.0 compared with `new` at 0.2.0, with `new`'s `#[moved_from]`.
fn compared(old: &str, new: &str) -> Report {
    let old = load_with("0.1.0", old, &keyed_deps());
    let (new_bundle, notes) = load_with_notes("0.2.0", new, &keyed_deps());
    let moves: Vec<Move> = notes.moved_from.iter().map(Move::from).collect();
    compare_moved(&old, &new_bundle, &moves).unwrap()
}

fn messages(report: &Report) -> String {
    report
        .breaking
        .iter()
        .map(|f| f.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

const API: &str = r#"
#[ww_api_root(wire = "w1.0")]
pub trait Api {
    ww_impl!(flow[]: Flow);
    ww_impl!(port[]: Flow);
    keyed!(flow by FlowKey);
}

#[ww_trait]
pub trait Flow {
    fn close();
}

#[derive_shrink_wrap]
pub struct FlowKey {
    pub src: u16,
}
"#;

/// The API without its `keyed!` line.
fn unkeyed(src: &str) -> String {
    src.replace("    keyed!(flow by FlowKey);\n", "")
}

#[test]
fn added_is_compatible() {
    let report = compared(&unkeyed(API), API);
    assert_eq!(report.change, Change::Compatible, "{report:?}");
    assert!(
        report
            .compatible
            .iter()
            .any(|c| c == "Api::flow_keys added"),
        "{:?}",
        report.compatible
    );
    assert_eq!(report.wire_bump, WireBump::Revision, "{report:?}");
}

#[test]
fn removed_is_breaking() {
    let report = compared(API, &unkeyed(API));
    assert_eq!(report.change, Change::Breaking, "{report:?}");
    assert_eq!(report.breaking[0].rule, Rule::Removed, "{report:?}");
    assert!(
        messages(&report).contains("Api::flow_keys removed"),
        "{report:?}"
    );
}

#[test]
fn unchanged_is_none() {
    let report = compared(API, API);
    assert!(report.breaking.is_empty(), "{report:?}");
    assert!(report.compatible.is_empty(), "{report:?}");
}

/// A key read by the other version as another type breaks the lookups both ways.
#[test]
fn key_type_changed_is_breaking() {
    let new = API.replace("pub src: u16,", "pub src: u32,");
    let report = compared(API, &new);
    assert_eq!(report.change, Change::Breaking, "{report:?}");
    let messages = messages(&report);
    assert!(messages.contains("Api::flow_keys: key type:"), "{messages}");
}

/// A field appended with a default evolves the key as any unsized struct.
#[test]
fn key_field_appended_with_default_is_compatible() {
    let new = API.replace(
        "pub src: u16,",
        "pub src: u16,\n    #[default = 0]\n    pub dst: u16,",
    );
    let report = compared(API, &new);
    assert!(report.breaking.is_empty(), "{report:?}");
}

/// Within a line the keyed array is its position: keying another one is breaking.
#[test]
fn keying_another_array_is_breaking() {
    let new = API.replace("keyed!(flow by FlowKey)", "keyed!(port by FlowKey)");
    let report = compared(API, &new);
    let messages = messages(&report);
    assert!(
        messages.contains("Api::flow_keys: keys flow on the old version, port on the new version"),
        "{messages}"
    );
}

const W2: &str = r#"
#[ww_api_root(wire = "w2.0")]
pub trait Api {
    ww_impl!(flow[]: Flow);
    ww_impl!(port[]: Flow);
    keyed!(flow by FlowKey);
}

#[ww_trait]
pub trait Flow {
    fn close();
}

#[derive_shrink_wrap]
pub struct FlowKey {
    pub src: u16,
}
"#;

/// Across a break the array is its path: the companion follows it to its new position, a wire change only.
#[test]
fn follows_an_array_at_another_position() {
    let new = W2.replace("wire = \"w2.0\"", "wire = \"w3.0\"").replace(
        "    ww_impl!(flow[]: Flow);\n    ww_impl!(port[]: Flow);\n",
        "    ww_impl!(port[]: Flow);\n    ww_impl!(flow[]: Flow);\n",
    );
    let report = compared(W2, &new);
    assert!(report.breaking.is_empty(), "{report:?}");
    assert_eq!(report.wire_bump, WireBump::Line, "{report:?}");
}

/// Across a break, an array renamed with `#[moved_from]` is still the one keyed.
#[test]
fn follows_a_moved_array() {
    let new = W2
        .replace("wire = \"w2.0\"", "wire = \"w3.0\"")
        .replace(
            "    ww_impl!(flow[]: Flow);\n",
            "    #[moved_from(w2.0, Api::flow)]\n    ww_impl!(conn[]: Flow);\n",
        )
        .replace("keyed!(flow by FlowKey)", "keyed!(conn by FlowKey)");
    let report = compared(W2, &new);
    assert!(report.moved_from_errors.is_empty(), "{report:?}");
    let messages = messages(&report);
    assert!(
        messages.contains("Api::flow moved to Api::conn"),
        "{messages}"
    );
    assert!(!messages.contains("keys"), "{messages}");
}

/// Across a break, the companion of another array is another resource: the old one is removed.
#[test]
fn keying_another_array_across_a_break_is_breaking() {
    let new = W2
        .replace("wire = \"w2.0\"", "wire = \"w3.0\"")
        .replace("keyed!(flow by FlowKey)", "keyed!(port by FlowKey)");
    let report = compared(W2, &new);
    assert_eq!(report.breaking[0].rule, Rule::Removed, "{report:?}");
    assert!(
        messages(&report).contains("Api::flow_keys removed (the keyed! companion of flow)"),
        "{report:?}"
    );
    assert!(
        report
            .compatible
            .iter()
            .any(|c| c == "Api::port_keys added (the keyed! companion of port)"),
        "{:?}",
        report.compatible
    );
}

/// `ww api diff` names the array a companion keys, as it is written.
#[test]
fn diff_names_the_keyed_array() {
    let old = load_with("0.1.0", &unkeyed(API), &keyed_deps());
    let keyed = load_with("0.1.0", API, &keyed_deps());
    assert_eq!(
        diff(&old, &keyed).unwrap(),
        [
            Difference::Added {
                path: "Api::flow_keys".into(),
                definition: "keyed!(flow by FlowKey)".into(),
            },
            Difference::Added {
                path: "dependency ww_keyed".into(),
                definition: "0.1.0".into(),
            }
        ]
    );
    // another key type is a change of the companion's own line
    let new = load_with(
        "0.1.0",
        &API.replace("keyed!(flow by FlowKey)", "keyed!(flow by Port)")
            .replace("pub struct FlowKey", "pub struct Port"),
        &keyed_deps(),
    );
    let changed = diff(&keyed, &new).unwrap();
    assert!(
        changed.contains(&Difference::Changed {
            path: "Api::flow_keys".into(),
            what: "signature",
            old: "keyed!(flow by FlowKey)".into(),
            new: "keyed!(flow by Port)".into(),
        }),
        "{changed:?}"
    );
}
