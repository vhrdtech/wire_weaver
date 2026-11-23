//! SRV-76: a mount slot is a resource of its own, added and removed as any other. Its presence type is checked, the
//! API crate it expects is a hint, and what is mounted there is not part of the API.

use super::*;

/// A crate depending on `ww_api` (`MountPresence` comes from there) and on `ww_keyed` (ww_base's checkout
/// `Cargo.lock` pins), as an API with `mount!` and `mounts!` must.
fn keyed_deps() -> String {
    let ww_api = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ww_api");
    format!(
        "ww_api = {{ path = {:?} }}\nww_keyed = {{ path = {:?} }}\n",
        ww_api.canonicalize().unwrap(),
        base_dir("ww_keyed").canonicalize().unwrap()
    )
}

fn compared(old: &str, new: &str) -> Report {
    let old = load_with("0.1.0", old, &keyed_deps());
    let new = load_with("0.2.0", new, &keyed_deps());
    compare(&old, &new).unwrap()
}

const API: &str = r#"
#[ww_api_root(wire = "w1.0")]
#[effect(edit)]
pub trait Api {
    mount!(hub: hub_api);
    mounts!(ext);
    fn mute();
}
"#;

fn without(line: &str) -> String {
    API.replace(&format!("    {line}\n"), "")
}

#[test]
fn added_is_compatible() {
    let report = compared(
        &without("fn mute();"),
        &API.replace("fn mute();", "fn mute();\n    mount!(radio);"),
    );
    assert_eq!(report.change, Change::Compatible, "{report:?}");
    assert!(
        report.compatible.iter().any(|c| c == "Api::radio added"),
        "{:?}",
        report.compatible
    );
}

#[test]
fn removed_is_breaking() {
    let report = compared(API, &without("mount!(hub: hub_api);"));
    assert_eq!(report.change, Change::Breaking, "{report:?}");
}

/// The expected API crate is a hint for clients: another one, or none, changes nothing on the wire.
#[test]
fn expected_crate_is_a_hint() {
    let report = compared(
        API,
        &API.replace("mount!(hub: hub_api)", "mount!(hub: other_api)"),
    );
    assert_ne!(report.change, Change::Breaking, "{report:?}");
    let report = compared(API, &API.replace("mount!(hub: hub_api)", "mount!(hub)"));
    assert_ne!(report.change, Change::Breaking, "{report:?}");
}

/// A slot where a method was, or the other way round, is breaking.
#[test]
fn slot_for_a_method_is_breaking() {
    let report = compared(API, &API.replace("fn mute();", "mount!(mute);"));
    assert_eq!(report.change, Change::Breaking, "{report:?}");
}

/// `ww api diff` writes slots as they are declared.
#[test]
fn diff_names_the_slots() {
    let old = load_with("0.1.0", &without("mounts!(ext);"), &keyed_deps());
    let appended = API
        .replace("    mounts!(ext);\n", "")
        .replace("fn mute();", "fn mute();\n    mounts!(ext);");
    let added = diff(&old, &load_with("0.1.0", &appended, &keyed_deps())).unwrap();
    let new = load_with("0.1.0", API, &keyed_deps());
    assert!(
        added.contains(&Difference::Added {
            path: "Api::ext".into(),
            definition: "mounts!(ext)".into(),
        }),
        "{added:?}"
    );
    let hub = load_with(
        "0.1.0",
        &API.replace("mount!(hub: hub_api)", "mount!(hub)"),
        &keyed_deps(),
    );
    let changed = diff(&new, &hub).unwrap();
    assert!(
        changed.contains(&Difference::Changed {
            path: "Api::hub".into(),
            what: "signature",
            old: "mount!(hub: hub_api)".into(),
            new: "mount!(hub)".into(),
        }),
        "{changed:?}"
    );
}
