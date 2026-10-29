//! [Check] on throwaway API crates with a saved snapshot, as an API crate's own evolution test runs it.

use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::SNAPSHOTS_DIR;
use wire_weaver_evolution::{Change, Check, Outcome, Rule, SaveStatus};

const API: &str = r#"
#[ww_api_root]
pub trait Api {
    fn led_on();
    fn led_off();
}

#[derive_shrink_wrap]
pub struct Status {
    pub on: bool,
}
"#;

/// A crate in a fresh directory, removed on drop.
struct TestCrate {
    dir: PathBuf,
}

impl TestCrate {
    fn new(name: &str) -> Self {
        // under the workspace, so the walker finds its Cargo.lock and [patch]es for git dependencies (exact_units)
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/ww_evolution_check_tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        TestCrate { dir }
    }

    fn write(&self, version: &str, lib_rs: &str) -> &Self {
        std::fs::write(
            self.dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\n\n[dependencies]\n"
            ),
        )
        .unwrap();
        let lib_rs = format!("use wire_weaver::prelude::*;\n{lib_rs}");
        std::fs::write(self.dir.join("src/lib.rs"), lib_rs).unwrap();
        self
    }

    /// Saves a snapshot of the current source, as `ww api save` does.
    fn save(&self) -> &Self {
        let bundle = wire_weaver_core::load_crate(&self.dir).unwrap();
        let version = bundle.crate_version(0).unwrap();
        let dir = self.dir.join(SNAPSHOTS_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        let text = wire_weaver_snapshots::render_ron(&bundle).unwrap();
        std::fs::write(
            dir.join(format!("{}.ron", version.filename_friendly())),
            text,
        )
        .unwrap();
        self
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TestCrate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn check(path: &Path, strict: bool) -> (Outcome, Result<String, String>) {
    let outcome = Check::new(path).strict(strict).run().unwrap();
    let verdict = outcome.verdict();
    (outcome, verdict)
}

#[test]
fn breaking_change_is_caught_with_rule_and_bump() {
    let c = TestCrate::new("breaking");
    c.write("0.1.0", API).save();
    let removed = API.replace("    fn led_off();\n", "");

    // changed without a bump: the released 0.1.0 never changes
    c.write("0.1.0", &removed);
    let (outcome, verdict) = check(c.path(), true);
    let Outcome::Compared(comparison) = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(comparison.report.change, Change::Breaking);
    assert_eq!(comparison.report.breaking[0].rule, Rule::Removed);
    let err = verdict.unwrap_err();
    for expected in [
        "breaking position (minor before 1.0, major after): 0.2.0 or later",
        "has a snapshot",
        "Bump `version` in",
        "to 0.2.0 or later",
        "ww api save",
    ] {
        assert!(err.contains(expected), "'{expected}' not in:\n{err}");
    }
    let report = outcome.to_string();
    for expected in [
        "Api::led_off removed",
        "rule: traits, resources and types are never removed",
        "docs/evolution/rules.md#api",
    ] {
        assert!(report.contains(expected), "'{expected}' not in:\n{report}");
    }

    // a compatible bump is not enough
    c.write("0.1.1", &removed);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(err.contains("0.2.0 or later"), "{err}");
    assert!(!err.contains("has a snapshot"), "{err}");

    c.write("0.2.0", &removed);
    assert_eq!(check(c.path(), true).1.unwrap(), "version is bumped enough");
}

#[test]
fn compatible_change_with_its_bump_passes() {
    let added = API.replace(
        "    fn led_off();\n",
        "    fn led_off();\n    #[since = \"{since}\"]\n    fn led_toggle();\n",
    );

    // before 1.0 the compatible position is patch
    let c = TestCrate::new("compatible-0x");
    c.write("0.1.0", API).save();
    let added_0x = added.replace("{since}", "0.1.1");
    c.write("0.1.0", &added_0x);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(
        err.contains("compatible position (patch before 1.0, minor after): 0.1.1 or later"),
        "{err}"
    );
    c.write("0.1.1", &added_0x);
    let (outcome, ok) = check(c.path(), true);
    assert_eq!(ok.unwrap(), "version is bumped enough");
    let Outcome::Compared(comparison) = outcome else {
        panic!()
    };
    assert_eq!(comparison.report.change, Change::Compatible);
    assert!(comparison.report.warnings.is_empty(), "{comparison}");

    // after 1.0 it is minor
    let c = TestCrate::new("compatible-1x");
    c.write("1.0.0", API).save();
    let added_1x = added.replace("{since}", "1.1.0");
    c.write("1.0.1", &added_1x);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(err.contains("1.1.0 or later"), "{err}");
    c.write("1.1.0", &added_1x);
    assert_eq!(check(c.path(), true).1.unwrap(), "version is bumped enough");
    Check::new(c.path()).strict(true).assert();

    // nothing changed since the snapshot of the current version
    let c = TestCrate::new("unchanged");
    c.write("0.1.0", API).save();
    assert_eq!(check(c.path(), true).1.unwrap(), "no changes");
}

#[test]
fn missing_snapshot_fails_in_strict_mode() {
    let c = TestCrate::new("missing");
    c.write("0.1.0", API);
    let (outcome, verdict) = check(c.path(), true);
    assert!(matches!(outcome, Outcome::NoSnapshot(_)), "{outcome:?}");
    let err = verdict.unwrap_err();
    for expected in [
        "no snapshot of test_api 0.1.0 or earlier",
        "strict mode",
        "ww api save",
    ] {
        assert!(err.contains(expected), "'{expected}' not in:\n{err}");
    }
    // without strict mode it passes, saying that nothing was checked
    let ok = Check::new(c.path()).run().unwrap().verdict().unwrap();
    assert!(ok.contains("nothing to compare with"), "{ok}");
}

#[test]
#[should_panic(expected = "strict mode")]
fn assert_panics_in_strict_mode_without_snapshot() {
    let c = TestCrate::new("assert-missing");
    c.write("0.1.0", API);
    Check::new(c.path()).strict(true).assert();
}

fn check_current(path: &Path) -> (Outcome, Result<String, String>) {
    let outcome = Check::new(path).require_current(true).run().unwrap();
    let verdict = outcome.verdict();
    (outcome, verdict)
}

fn has_all(text: &str, expected: &[&str]) {
    for expected in expected {
        assert!(text.contains(expected), "'{expected}' not in:\n{text}");
    }
}

#[test]
fn require_current_fails_without_the_current_versions_snapshot() {
    let c = TestCrate::new("current-missing");
    c.write("0.1.0", API);
    let (outcome, verdict) = check_current(c.path());
    assert!(matches!(outcome, Outcome::NoSnapshot(_)), "{outcome:?}");
    has_all(
        &verdict.unwrap_err(),
        &[
            "no snapshot of test_api 0.1.0 in",
            "current version's snapshot is required",
            "just api-save",
            "WW_API_SAVE=1 cargo test --test evolution",
        ],
    );

    // the previous version's snapshot is not enough: 0.1.1 shipped without one is the gap this closes
    c.save();
    c.write("0.1.1", API);
    let err = check_current(c.path()).1.unwrap_err();
    has_all(
        &err,
        &[
            "no snapshot of test_api 0.1.1",
            "the previous one is",
            "test_api_0_1_0.ron",
            "just api-save",
        ],
    );
    // without require_current the same crate passes against 0.1.0
    assert_eq!(check(c.path(), true).1.unwrap(), "no changes");
}

#[test]
fn require_current_fails_when_source_changed_after_save() {
    let c = TestCrate::new("current-changed");
    c.write("0.1.0", API).save();
    c.write("0.1.1", API).save();
    assert_eq!(check_current(c.path()).1.unwrap(), "no changes");

    let added = API.replace(
        "    fn led_off();\n",
        "    fn led_off();\n    #[since = \"0.1.1\"]\n    fn led_toggle();\n",
    );
    c.write("0.1.1", &added);
    let (outcome, verdict) = check_current(c.path());
    let Outcome::Compared(comparison) = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(comparison.new_label, "source");
    assert!(comparison.old_path.ends_with("test_api_0_1_1.ron"));
    has_all(
        &verdict.unwrap_err(),
        &[
            "The source differs from",
            "test_api_0_1_1.ron, the snapshot of test_api 0.1.1",
            "If test_api 0.1.1 is not released yet, save it again with `just api-save`",
            "never changes",
        ],
    );

    // saved again, the snapshot of 0.1.1 is compared with 0.1.0's
    let saved = Check::new(c.path()).save(false).unwrap();
    assert_eq!(saved.status, SaveStatus::Refreshed);
    let (outcome, ok) = check_current(c.path());
    assert_eq!(ok.unwrap(), "version is bumped enough");
    let Outcome::Compared(comparison) = &outcome else {
        panic!("{outcome:?}")
    };
    assert!(comparison.old_path.ends_with("test_api_0_1_0.ron"));
    assert!(comparison.new_label.ends_with("test_api_0_1_1.ron"));
    assert_eq!(comparison.report.change, Change::Compatible);
}

#[test]
fn save_writes_only_the_current_version_and_refuses_a_bump_too_small() {
    let c = TestCrate::new("save");
    c.write("0.1.0", API);
    let saved = Check::new(c.path()).save(false).unwrap();
    assert_eq!(saved.status, SaveStatus::Saved);
    assert!(saved.previous.is_none());
    let released = std::fs::read_to_string(&saved.path).unwrap();
    assert_eq!(
        Check::new(c.path()).save(false).unwrap().status,
        SaveStatus::UpToDate
    );

    // breaking change with a patch bump: refused, nothing written
    let removed = API.replace("    fn led_off();\n", "");
    c.write("0.1.1", &removed);
    let err = Check::new(c.path()).save(false).unwrap_err();
    has_all(&err, &["0.2.0 or later", "Not saved", "Cargo.toml"]);
    let dir = c.path().join(SNAPSHOTS_DIR);
    assert!(!dir.join("test_api_0_1_1.ron").exists());
    // --force saves anyway
    assert_eq!(
        Check::new(c.path()).save(true).unwrap().status,
        SaveStatus::Saved
    );

    c.write("0.2.0", &removed);
    let saved = Check::new(c.path()).save(false).unwrap();
    assert_eq!(saved.status, SaveStatus::Saved);
    let (previous, report) = saved.previous.as_ref().unwrap();
    assert!(previous.ends_with("test_api_0_1_1.ron"), "{previous:?}");
    assert_eq!(report.change, Change::None);
    // older versions' snapshots are never written
    assert_eq!(
        std::fs::read_to_string(dir.join("test_api_0_1_0.ron")).unwrap(),
        released
    );
}

#[test]
fn compatible_bump_with_current_snapshot_passes() {
    let c = TestCrate::new("current-compatible");
    c.write("1.0.0", API).save();
    let added = API.replace(
        "    fn led_off();\n",
        "    fn led_off();\n    #[since = \"1.1.0\"]\n    fn led_toggle();\n",
    );
    c.write("1.1.0", &added).save();
    let (outcome, ok) = check_current(c.path());
    assert_eq!(ok.unwrap(), "version is bumped enough");
    let Outcome::Compared(comparison) = outcome else {
        panic!()
    };
    assert_eq!(comparison.report.change, Change::Compatible);
    Check::new(c.path()).require_current(true).assert();

    // a snapshot of a version not bumped enough (saved with force) fails, naming the version to bump to
    c.write("1.1.1", &API.replace("    fn led_off();\n", ""))
        .save();
    let err = check_current(c.path()).1.unwrap_err();
    has_all(
        &err,
        &[
            "2.0.0 or later",
            "test_api_1_1_1.ron goes then, as test_api 1.1.1 was never released",
        ],
    );
}

/// nucleo_g0b1re_template's API 0.4.0 with its snapshots, saved by a wire_weaver from before
/// `ItemEnum::future_version` (EVO-10).
fn old_format_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nucleo_g0b1re_api")
}

/// The fixture copied into `c`, its path dependencies made absolute, with `lib_rs` changed by `edit`.
fn copy_fixture(c: &TestCrate, edit: impl Fn(String) -> String) {
    let fixture = old_format_fixture();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo_toml = std::fs::read_to_string(fixture.join("Cargo.toml"))
        .unwrap()
        .replace(
            "path = \"../../../../",
            &format!("path = \"{}/", repo.display()),
        );
    std::fs::write(c.path().join("Cargo.toml"), cargo_toml).unwrap();
    let lib_rs = std::fs::read_to_string(fixture.join("src/lib.rs")).unwrap();
    std::fs::write(c.path().join("src/lib.rs"), edit(lib_rs)).unwrap();
    let snapshots = c.path().join(SNAPSHOTS_DIR);
    std::fs::create_dir_all(&snapshots).unwrap();
    for entry in std::fs::read_dir(fixture.join(SNAPSHOTS_DIR)).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, snapshots.join(path.file_name().unwrap())).unwrap();
    }
}

fn unrecorded(outcome: &Outcome) -> Vec<String> {
    let Outcome::Compared(comparison) = outcome else {
        panic!("{outcome:?}")
    };
    comparison
        .report
        .unrecorded
        .iter()
        .map(|u| u.to_string())
        .collect()
}

#[test]
fn old_format_snapshot_passes_against_unchanged_source() {
    // upgrading wire_weaver alone is no change of the API (EVO-13): the snapshot has no future_version, which codegen
    // now records for the crate's SiliconRevision
    let (outcome, verdict) = check_current(&old_format_fixture());
    verdict.unwrap_or_else(|e| panic!("{outcome}\n{e}"));

    let snapshot = old_format_fixture()
        .join(SNAPSHOTS_DIR)
        .join("nucleo_g0b1re_api_0_4_0.ron");
    let outcome = Check::new(old_format_fixture())
        .against(&snapshot)
        .run()
        .unwrap();
    assert_eq!(outcome.verdict().unwrap(), "no changes", "{outcome}");
    // ItemStruct::extra (DER-18 transparent, INT-13 marker) came later still
    assert_eq!(
        unrecorded(&outcome),
        ["ItemEnum::future_version", "ItemStruct::extra"]
    );
    has_all(
        &outcome.to_string(),
        &[
            "not compared: ItemEnum::future_version, ItemStruct::extra, which a snapshot saved by an older \
            wire_weaver",
        ],
    );
}

#[test]
fn real_change_against_old_format_snapshot_is_caught() {
    let c = TestCrate::new("old-format-change");
    copy_fixture(&c, |lib_rs| lib_rs.replace("    fn led_off();\n", ""));
    let (outcome, verdict) = check_current(c.path());
    let Outcome::Compared(comparison) = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(comparison.report.change, Change::Breaking, "{outcome}");
    has_all(
        &verdict.unwrap_err(),
        &["0.5.0 or later", "The source differs from"],
    );

    // and a compatible one: a variant appended to the enum with the catch-all, which the old snapshot can't tell
    // had one, so the bump is breaking as for an enum without (docs/evolution/rules.md#unsized-types)
    let c = TestCrate::new("old-format-variant");
    copy_fixture(&c, |lib_rs| {
        lib_rs.replace(
            "    Unknown,\n}",
            "    Unknown,\n    /// Revision B\n    B,\n}",
        )
    });
    let (outcome, verdict) = check_current(c.path());
    assert!(verdict.is_err(), "{outcome}");
    let Outcome::Compared(comparison) = &outcome else {
        panic!("{outcome:?}")
    };
    assert_ne!(comparison.report.change, Change::None, "{outcome}");
}

#[test]
fn any_field_a_snapshot_lacks_is_not_recorded() {
    // a snapshot without fields of the model, as one saved before they were added: not a change, for any field
    let api = format!(
        "{API}\n#[derive_shrink_wrap(ww_repr = u4)]\npub enum Mode {{\n    Off,\n    On,\n}}\n"
    );
    let c = TestCrate::new("unrecorded-fields");
    c.write("0.1.0", &api).save();
    let path = c.path().join(SNAPSHOTS_DIR).join("test_api_0_1_0.ron");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut stripped = text.clone();
    for field in ["future_version", "instance_of"] {
        stripped = strip_field(&stripped, field);
    }
    assert_ne!(stripped, text);
    std::fs::write(&path, &stripped).unwrap();

    let (outcome, verdict) = check_current(c.path());
    verdict.unwrap_or_else(|e| panic!("{outcome}\n{e}"));
    let outcome = Check::new(c.path()).against(&path).run().unwrap();
    assert_eq!(outcome.verdict().unwrap(), "no changes", "{outcome}");
    assert_eq!(
        unrecorded(&outcome),
        [
            "ItemEnum::future_version",
            "ItemEnum::instance_of",
            "ItemStruct::instance_of"
        ]
    );

    // a real change is still one
    c.write("0.1.0", &api.replace("    On,\n", "    On,\n    Auto,\n"));
    let (outcome, verdict) = check_current(c.path());
    assert!(verdict.is_err(), "{outcome}");

    // saving it again records every field, and leaves an unchanged one up to date
    c.write("0.1.0", &api);
    let saved = Check::new(c.path()).save(false).unwrap();
    assert_eq!(saved.status, SaveStatus::Reformatted);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(
        Check::new(c.path()).save(false).unwrap().status,
        SaveStatus::UpToDate
    );
}

/// TOOL-22: a plain integer field whose name ends in a known unit suffix is noted, never an error, and the note
/// names the `exact_units::Fixed` it could become and the name without its suffix; silenced per field by
/// `#[no_unit_hint]`.
#[test]
fn unit_suffix_hint_is_noted_and_silenced() {
    let api = r#"
#[derive_shrink_wrap]
pub struct SteerPoint {
    pub shared_us: u64,
}
"#;
    let c = TestCrate::new("unit-hint");
    c.write("0.1.0", api);
    let (outcome, _) = check(c.path(), false);
    let hints = outcome.hints();
    assert_eq!(hints.len(), 1, "{hints:?}");
    let hint = hints[0].to_string();
    for expected in [
        "SteerPoint::shared_us",
        "exact_units::Fixed<Second, Micro, u64>",
        "exact_units::Micros<u64>",
        "renamed to `shared`",
        "#[no_unit_hint]",
    ] {
        assert!(hint.contains(expected), "'{expected}' not in: {hint}");
    }
    assert!(outcome.to_string().contains(&hint));

    let silenced = api.replace(
        "    pub shared_us: u64,",
        "    #[no_unit_hint]\n    pub shared_us: u64,",
    );
    c.write("0.1.0", &silenced);
    let (outcome, _) = check(c.path(), false);
    assert!(outcome.hints().is_empty(), "{:?}", outcome.hints());
}

/// TOOL-22 also hints at a method argument and a property by name, silenced the same way: bare `#[no_unit_hint]`
/// on the method silences every argument, naming arguments silences only those.
#[test]
fn unit_suffix_hint_on_arguments_and_properties() {
    let api = r#"
#[ww_api_root]
pub trait Api {
    fn configure(timeout_ms: u32, retries: u8);
    property!(ro rate_hz: u32);
}
"#;
    let c = TestCrate::new("unit-hint-api");
    c.write("0.1.0", api);
    let (outcome, _) = check(c.path(), false);
    let names: Vec<_> = outcome.hints().iter().map(|h| h.item.clone()).collect();
    assert_eq!(names, vec!["configure::timeout_ms", "rate_hz"], "{names:?}");

    let silenced = api
        .replace(
            "    fn configure(timeout_ms: u32, retries: u8);",
            "    #[no_unit_hint]\n    fn configure(timeout_ms: u32, retries: u8);",
        )
        .replace(
            "    property!(ro rate_hz: u32);",
            "    #[no_unit_hint]\n    property!(ro rate_hz: u32);",
        );
    c.write("0.1.0", &silenced);
    let (outcome, _) = check(c.path(), false);
    assert!(outcome.hints().is_empty(), "{:?}", outcome.hints());
}

/// `text` without `, field: <value>` anywhere, the value up to the next `,` or closing bracket at its depth.
fn strip_field(text: &str, field: &str) -> String {
    let needle = format!(", {field}: ");
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        out.push_str(&rest[..at]);
        let value = &rest[at + needle.len()..];
        let mut depth = 0i32;
        let mut in_string = false;
        let end = value
            .char_indices()
            .find(|&(_, ch)| {
                match ch {
                    '"' => in_string = !in_string,
                    '(' | '[' if !in_string => depth += 1,
                    ')' | ']' if !in_string => depth -= 1,
                    _ => {}
                }
                !in_string && (depth < 0 || (depth == 0 && ch == ','))
            })
            .map(|(i, _)| i)
            .unwrap();
        rest = &value[end..];
    }
    out.push_str(rest);
    out
}
