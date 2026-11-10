//! [Check] on throwaway API crates with a saved snapshot, as an API crate's own evolution test runs it.

use std::path::{Path, PathBuf};
use wire_weaver_evolution::snapshot_files::SNAPSHOTS_DIR;
use wire_weaver_evolution::{
    Change, Check, CrateCache, Outcome, RootsOutcome, Rule, SaveStatus, Saved,
};

const API: &str = r#"
#[ww_api_root(wire = "w1.0")]
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
        self.write_with_deps(version, lib_rs, "")
    }

    fn write_with_deps(&self, version: &str, lib_rs: &str, dependencies: &str) -> &Self {
        std::fs::write(
            self.dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{dependencies}"
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

    /// A crate with several API roots (EVO-37): `roots` as `[package.metadata.wire_weaver.roots]`, `modules` as
    /// `src/<name>.rs`, each `pub mod` of `lib.rs`.
    fn write_roots(
        &self,
        version: &str,
        roots: &[(&str, &str)],
        modules: &[(&str, &str)],
    ) -> &Self {
        let roots: String = roots
            .iter()
            .map(|(root, wire)| format!("\"{root}\" = \"{wire}\"\n"))
            .collect();
        std::fs::write(
            self.dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n\n\
                 [package.metadata.wire_weaver.roots]\n{roots}\n[dependencies]\n"
            ),
        )
        .unwrap();
        let mut lib_rs = String::new();
        for (name, source) in modules {
            lib_rs.push_str(&format!("pub mod {name};\n"));
            let source = format!("use wire_weaver::prelude::*;\n{source}");
            std::fs::write(self.dir.join(format!("src/{name}.rs")), source).unwrap();
        }
        std::fs::write(self.dir.join("src/lib.rs"), lib_rs).unwrap();
        self
    }

    fn snapshot(&self, file_name: &str) -> PathBuf {
        self.dir.join(SNAPSHOTS_DIR).join(file_name)
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

/// [Check::save] of a crate with one snapshot (no `roots` table).
fn save_one(path: &Path, force: bool) -> Saved {
    let mut saved = Check::new(path).save(force).unwrap();
    assert_eq!(saved.len(), 1, "{saved:?}");
    saved.remove(0)
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

    // a wire break needs a new wire line too (EVO-27)
    c.write("0.2.0", &removed);
    let err = check(c.path(), true).1.unwrap_err();
    for expected in [
        "test_api wire w1.0 -> w1.0 has a wire break (Api::led_off removed), which needs a new wire line: w2.0 or later",
        "Bump the wire version in `#[ww_api_root(wire = ..)]` to w2.0 or later",
    ] {
        assert!(err.contains(expected), "'{expected}' not in:\n{err}");
    }
    assert!(!err.contains("0.2.0 or later"), "{err}");
    c.write(
        "0.2.0",
        &removed.replace("wire = \"w1.0\"", "wire = \"w2.0\""),
    );
    assert_eq!(check(c.path(), true).1.unwrap(), "version is bumped enough");
}

#[test]
fn compatible_change_with_its_bump_passes() {
    // the wire revision counts additions on its own, whatever the crate version (EVO-24)
    let added = API
        .replace(
            "    fn led_off();\n",
            "    fn led_off();\n    #[since = \"w1.1\"]\n    fn led_toggle();\n",
        )
        .replace("wire = \"w1.0\"", "wire = \"w1.1\"");

    // before 1.0 the compatible position is patch
    let c = TestCrate::new("compatible-0x");
    c.write("0.1.0", API).save();
    let added_0x = &added;
    c.write("0.1.0", added_0x);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(
        err.contains("compatible position (patch before 1.0, minor after): 0.1.1 or later"),
        "{err}"
    );
    c.write("0.1.1", added_0x);
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
    let added_1x = &added;
    c.write("1.0.1", added_1x);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(err.contains("1.1.0 or later"), "{err}");
    c.write("1.1.0", added_1x);
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

    let added = API
        .replace(
            "    fn led_off();\n",
            "    fn led_off();\n    #[since = \"w1.1\"]\n    fn led_toggle();\n",
        )
        .replace("wire = \"w1.0\"", "wire = \"w1.1\"");
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
    let saved = save_one(c.path(), false);
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
    let saved = save_one(c.path(), false);
    assert_eq!(saved.status, SaveStatus::Saved);
    assert!(saved.previous.is_none());
    let released = std::fs::read_to_string(&saved.path).unwrap();
    assert_eq!(save_one(c.path(), false).status, SaveStatus::UpToDate);

    // breaking change with a patch bump: refused, nothing written
    let removed = API.replace("    fn led_off();\n", "");
    c.write("0.1.1", &removed);
    let err = Check::new(c.path()).save(false).unwrap_err();
    has_all(&err, &["0.2.0 or later", "Not saved", "Cargo.toml"]);
    let dir = c.path().join(SNAPSHOTS_DIR);
    assert!(!dir.join("test_api_0_1_1.ron").exists());
    // --force saves anyway
    assert_eq!(save_one(c.path(), true).status, SaveStatus::Saved);

    c.write("0.2.0", &removed);
    let saved = save_one(c.path(), false);
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
    let added = API
        .replace(
            "    fn led_off();\n",
            "    fn led_off();\n    #[since = \"w1.1\"]\n    fn led_toggle();\n",
        )
        .replace("wire = \"w1.0\"", "wire = \"w1.1\"");
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
    // ItemStruct::extra (DER-18 transparent, INT-13 marker) came later still, ApiBundle::wire (EVO-24) and
    // ApiBundle::ext_wire (EVO-26) last
    assert_eq!(
        unrecorded(&outcome),
        [
            "ApiBundle::ext_wire",
            "ApiBundle::wire",
            "ItemEnum::future_version",
            "ItemStruct::extra"
        ]
    );
    has_all(
        &outcome.to_string(),
        &[
            "not compared: ApiBundle::ext_wire, ApiBundle::wire, ItemEnum::future_version, ItemStruct::extra, which a \
            snapshot saved by an older wire_weaver",
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
    let saved = save_one(c.path(), false);
    assert_eq!(saved.status, SaveStatus::Reformatted);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(save_one(c.path(), false).status, SaveStatus::UpToDate);
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
#[ww_api_root(wire = "w1.0")]
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

/// A tiny path-dependency crate with one struct, whose own version and `id` field type `write_dep` sets: a stand-in
/// for a `ww_stdlib` crate making a breaking change of its own (EVO-21, as `ww_board_info`'s `Rework` did, STD-98).
fn write_dep(name: &str, version: &str, id_ty: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/ww_evolution_check_tests_dep")
        .join(format!("{name}-{version}-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"dep_api\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        format!(
            "use wire_weaver::prelude::*;\n\n#[derive_shrink_wrap]\npub struct Shared {{\n    pub id: {id_ty},\n}}\n"
        ),
    )
    .unwrap();
    dir
}

/// EVO-21: `dep_api` (a stand-in for a `ww_stdlib` crate) bumps its own breaking position underneath an unchanged
/// API crate that uses its type in two different methods: the report groups both breaks under the one dependency
/// type that actually changed, instead of listing two unrelated-looking breaks.
#[test]
fn shared_dependency_type_is_named_once_for_every_item_it_reaches() {
    let api = r#"
use dep_api::Shared;

#[ww_api_root(wire = "w1.0")]
pub trait Api {
    fn one(s: Shared);
    fn two(s: Shared);
}
"#;
    let c = TestCrate::new("root-cause");
    let old_dep = write_dep("shared", "0.1.0", "u8");
    let deps = format!(
        "dep_api = {{ path = {:?} }}\n",
        old_dep.canonicalize().unwrap()
    );
    c.write_with_deps("0.1.0", api, &deps).save();

    let new_dep = write_dep("shared", "0.2.0", "u16");
    let deps = format!(
        "dep_api = {{ path = {:?} }}\n",
        new_dep.canonicalize().unwrap()
    );
    c.write_with_deps("0.1.0", api, &deps);

    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_err(), "{outcome}");
    let text = outcome.to_string();
    for expected in [
        "2 broken because type dep_api::Shared changed:",
        "breaking: Api::one:",
        "breaking: Api::two:",
    ] {
        assert!(text.contains(expected), "'{expected}' not in:\n{text}");
    }
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

/// EVO-27: one check derives both bumps; each one not bumped enough is named with what to change, both at once when
/// both are missing, and the report says what is needed in `w2.3` form.
#[test]
fn both_bumps_named_in_the_verdict() {
    let c = TestCrate::new("both-bumps");
    c.write("0.1.0", API).save();
    // a trailing argument with a default: a new wire revision and a crate breaking bump
    let added = API.replace("fn led_on();", "fn led_on(brightness: Option<u8>);");

    c.write("0.1.1", &added);
    let (outcome, verdict) = check(c.path(), true);
    let err = verdict.unwrap_err();
    for expected in [
        "breaking position (minor before 1.0, major after): 0.2.0 or later",
        "test_api wire w1.0 -> w1.0 has Api::led_on: arguments added or removed at the end, which needs a new wire \
         revision: w1.1 or later",
        "Bump the wire version in `#[ww_api_root(wire = ..)]` to w1.1 or later",
        "docs/evolution/rules.md#the-wire-version",
    ] {
        assert!(err.contains(expected), "'{expected}' not in:\n{err}");
    }
    let report = outcome.to_string();
    let needs = "needs: wire w1.0 -> w1.1 or later (a new wire revision), crate 0.1.0 -> 0.2.0 or later (breaking \
                 position (minor before 1.0, major after))";
    assert!(report.contains(needs), "'{needs}' not in:\n{report}");

    // the crate bumped: only the wire is named
    c.write("0.2.0", &added);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(!err.contains("0.2.0 or later"), "{err}");
    assert!(err.contains("w1.1 or later"), "{err}");

    // both bumped
    c.write(
        "0.2.0",
        &added.replace("wire = \"w1.0\"", "wire = \"w1.1\""),
    );
    assert_eq!(check(c.path(), true).1.unwrap(), "version is bumped enough");

    // saving refuses a wire version not bumped enough, like a crate version
    c.write("0.2.0", &added);
    let err = Check::new(c.path()).save(false).unwrap_err();
    assert!(
        err.contains(
            "Not saved: bump the wire version in `#[ww_api_root(wire = ..)]` to w1.1 or later"
        ),
        "{err}"
    );
}

/// EVO-27: a crate declares its wire version in `Cargo.toml` (EVO-26; API roots too since EVO-31), and the hint
/// says so: once for a crate without an API root, once for a bare `#[ww_api_root]`.
#[test]
fn wire_version_of_a_crate_without_root_is_bumped_in_cargo_toml() {
    wire_version_in_cargo_toml_is_bumped("manifest-wire", "#[ww_trait]");
    wire_version_in_cargo_toml_is_bumped("manifest-wire-root", "#[ww_api_root]");
}

fn wire_version_in_cargo_toml_is_bumped(name: &str, marker: &str) {
    let shared = r#"
#[ww_trait]
pub trait Shared {
    fn get() -> u8;
}
"#;
    let shared = shared.replace("#[ww_trait]", marker);
    let shared = shared.as_str();
    let c = TestCrate::new(name);
    let write = |version: &str, wire: &str, lib_rs: &str| {
        c.write(version, lib_rs);
        let manifest = c.path().join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        let text = text.replace(
            "\n[dependencies]",
            &format!("\n[package.metadata.wire_weaver]\nwire = \"{wire}\"\n\n[dependencies]"),
        );
        std::fs::write(manifest, text).unwrap();
    };
    write("0.1.0", "w1.0", shared);
    c.save();
    let added = shared.replace("fn get() -> u8;", "fn get() -> u8;\n    fn set(v: u8);");
    write("0.1.1", "w1.0", &added);
    let err = check(c.path(), true).1.unwrap_err();
    for expected in [
        "test_api wire w1.0 -> w1.0 has Shared::set added, which needs a new wire revision: w1.1 or later",
        "Bump the wire version in `[package.metadata.wire_weaver] wire` of",
    ] {
        assert!(err.contains(expected), "'{expected}' not in:\n{err}");
    }
    write("0.1.1", "w1.1", &added);
    assert_eq!(check(c.path(), true).1.unwrap(), "version is bumped enough");
}

const COMMON: &str = r#"
#[derive_shrink_wrap]
pub struct Code(pub u16);

#[ww_trait]
pub trait Channel {
    fn read() -> Code;
}
"#;

const LINE_1: &str = r#"
use super::common::Code;
#[ww_api_root]
pub trait Api {
    fn read_once() -> Code;
    ww_impl!(channel: super::common::Channel);
}
"#;

const LINE_2: &str = r#"
use super::common::Code;
#[ww_api_root]
pub trait Api {
    fn read_once(average: u8) -> Code;
    ww_impl!(channel: super::common::Channel);
}
"#;

/// The names and statuses [Check::save] gave, by root.
fn saved_roots(path: &Path) -> Vec<(String, String, SaveStatus)> {
    Check::new(path)
        .save(false)
        .unwrap()
        .into_iter()
        .map(|s| {
            let file = s.path.file_name().unwrap().to_str().unwrap().to_string();
            (s.root.unwrap(), file, s.status)
        })
        .collect()
}

fn crate_version_in(path: &Path) -> String {
    let bundle = wire_weaver_evolution::snapshot_files::parse(path).unwrap();
    wire_weaver_evolution::snapshot_files::triplet(&bundle.ext_crates[0].version)
}

/// A crate with several roots (EVO-38): one snapshot per root and wire version, each holding its root and what it
/// reaches; a root that didn't change keeps its file and the crate version it was saved at, a changed one gets a new
/// file at its new wire version, checked against its previous one.
#[test]
fn snapshots_per_root() {
    let c = TestCrate::new("per_root");
    let roots = [("wire_v1::Api", "w1.0"), ("wire_v2::Api", "w2.0")];
    let modules = [("common", COMMON), ("wire_v1", LINE_1), ("wire_v2", LINE_2)];
    c.write_roots("0.1.0", &roots, &modules);
    let v1 = "test_api_0_1_0.wire_v1.Api@w1.0.ron";
    let v2 = "test_api_0_1_0.wire_v2.Api@w2.0.ron";
    assert_eq!(
        saved_roots(c.path()),
        [
            ("wire_v1::Api".into(), v1.into(), SaveStatus::Saved),
            ("wire_v2::Api".into(), v2.into(), SaveStatus::Saved),
        ]
    );
    let text = std::fs::read_to_string(c.snapshot(v2)).unwrap();
    assert!(
        text.starts_with("// test_api 0.1.0: API root wire_v2::Api at wire w2.0"),
        "{text}"
    );
    let bundle = wire_weaver_evolution::snapshot_files::parse(&c.snapshot(v2)).unwrap();
    assert_eq!(bundle.root.items.len(), 1);
    assert_eq!(bundle.wire, Some("w2.0".parse().unwrap()));
    let again = Check::new(c.path()).save(false).unwrap();
    assert!(again.iter().all(|s| s.status == SaveStatus::UpToDate));
    assert!(again.iter().all(|s| s.warnings.is_empty()), "{again:?}");
    // each root is checked against its own snapshot (EVO-39)
    let (outcome, verdict) = check(c.path(), true);
    assert_eq!(
        verdict.unwrap(),
        "`wire_v1::Api`: no changes; `wire_v2::Api`: no changes",
        "{outcome}"
    );

    // line 2 gets a method: a new revision of its line and a compatible crate bump; line 1 is unchanged and keeps
    // the file 0.1.0 saved
    let line_2b = LINE_2.replace("Channel);\n", "Channel);\n    fn stop();\n");
    let modules = [
        ("common", COMMON),
        ("wire_v1", LINE_1),
        ("wire_v2", &line_2b),
    ];
    let roots = [("wire_v1::Api", "w1.0"), ("wire_v2::Api", "w2.1")];
    c.write_roots("0.1.1", &roots, &modules);
    let saved = Check::new(c.path()).save(false).unwrap();
    assert_eq!(saved[0].status, SaveStatus::UpToDate);
    assert_eq!(
        saved[0].path,
        c.snapshot(v1),
        "an unchanged root keeps its file"
    );
    assert_eq!(saved[1].status, SaveStatus::Saved);
    let v2_1 = "test_api_0_1_1.wire_v2.Api@w2.1.ron";
    assert_eq!(saved[1].path, c.snapshot(v2_1));
    let (previous, report) = saved[1].previous.as_ref().unwrap();
    assert_eq!(previous, &c.snapshot(v2));
    assert_eq!(report.change, Change::Compatible);
    assert_eq!(crate_version_in(&c.snapshot(v2_1)), "0.1.1");
    assert!(!c.snapshot("test_api_0_1_1.wire_v1.Api@w1.0.ron").exists());

    // a doc comment on line 1: a compatible crate bump, no wire bump; a new file at 0.1.2, the released one of 0.1.0
    // never changes
    let line_1b = LINE_1.replace("#[ww_api_root]", "/// The first line.\n#[ww_api_root]");
    let modules = [
        ("common", COMMON),
        ("wire_v1", &line_1b),
        ("wire_v2", &line_2b),
    ];
    let released = std::fs::read_to_string(c.snapshot(v1)).unwrap();
    c.write_roots("0.1.2", &roots, &modules);
    let saved = saved_roots(c.path());
    let v1_2 = "test_api_0_1_2.wire_v1.Api@w1.0.ron";
    assert_eq!(saved[0].1, v1_2);
    assert_eq!(saved[0].2, SaveStatus::Saved);
    assert_eq!(saved[1].2, SaveStatus::UpToDate);
    assert_eq!(std::fs::read_to_string(c.snapshot(v1)).unwrap(), released);
    // the current version's own file is refreshed while it is worked on
    let line_1c = line_1b.replace("The first line.", "Line one.");
    let modules = [
        ("common", COMMON),
        ("wire_v1", &line_1c),
        ("wire_v2", &line_2b),
    ];
    c.write_roots("0.1.2", &roots, &modules);
    assert_eq!(
        saved_roots(c.path())[0],
        ("wire_v1::Api".into(), v1_2.into(), SaveStatus::Refreshed)
    );

    // line 2 breaks without a new line: refused, naming the root and its entry in the table, and nothing is written,
    // not even line 1's file
    let line_1d = line_1c.replace("Line one.", "Line 1.");
    let line_2c = line_2b.replace("    fn stop();\n", "");
    let modules = [
        ("common", COMMON),
        ("wire_v1", &line_1d),
        ("wire_v2", &line_2c),
    ];
    c.write_roots("0.2.0", &roots, &modules);
    let err = Check::new(c.path()).save(false).unwrap_err();
    for expected in [
        "API root `wire_v2::Api`",
        "`\"wire_v2::Api\"` of `[package.metadata.wire_weaver.roots]`",
        "w3.0",
    ] {
        assert!(err.contains(expected), "'{expected}' not in:\n{err}");
    }
    assert!(!c.snapshot("test_api_0_2_0.wire_v1.Api@w1.0.ron").exists());

    // an older crate version than a saved snapshot of the root is never saved
    c.write_roots(
        "0.1.0",
        &roots,
        &[
            ("common", COMMON),
            ("wire_v1", LINE_1),
            ("wire_v2", &line_2b),
        ],
    );
    let err = Check::new(c.path()).save(false).unwrap_err();
    assert!(err.contains("never written"), "{err}");
}

/// A crate that had one root and now has several (EVO-38): its whole-crate snapshots are the earlier snapshots of
/// the root on the same line with the same trait name.
#[test]
fn crate_snapshot_is_a_roots_previous_one() {
    let c = TestCrate::new("one_to_several");
    c.write("0.1.0", "#[ww_api_root(wire = \"w1.0\")]\npub trait Api {\n    fn led_on();\n    fn led_off();\n}\n#[derive_shrink_wrap]\npub struct Unused { pub a: u8 }\n")
        .save();
    let line_1 = "#[ww_api_root]\npub trait Api {\n    fn led_on();\n}\n";
    let line_2 = "#[ww_api_root]\npub trait Api {\n    fn blink();\n}\n";
    let roots = [("wire_v1::Api", "w1.0"), ("wire_v2::Api", "w2.0")];
    c.write_roots("0.2.0", &roots, &[("wire_v1", line_1), ("wire_v2", line_2)]);
    // `led_off` removed from line 1 without a new line; `Unused`, which no root reaches, is not compared
    let err = Check::new(c.path()).save(false).unwrap_err();
    assert!(err.contains("API root `wire_v1::Api`"), "{err}");
    assert!(err.contains("led_off"), "{err}");
    assert!(!err.contains("Unused"), "{err}");

    let line_1 = "#[ww_api_root]\npub trait Api {\n    fn led_on();\n    fn led_off();\n}\n";
    c.write_roots("0.2.0", &roots, &[("wire_v1", line_1), ("wire_v2", line_2)]);
    let saved = Check::new(c.path()).save(false).unwrap();
    // line 1 is what 0.1.0 saved as the whole crate: that file stays its latest snapshot
    assert_eq!(saved[0].status, SaveStatus::UpToDate);
    assert_eq!(saved[0].path, c.snapshot("test_api_0_1_0.ron"));
    let (previous, report) = saved[0].previous.as_ref().unwrap();
    assert_eq!(previous, &c.snapshot("test_api_0_1_0.ron"));
    assert_eq!(report.change, Change::None);
    assert!(saved[0].warnings.is_empty(), "{:?}", saved[0].warnings);
    // line 2 has no earlier snapshot: the crate's was on line 1, which the warning says
    assert_eq!(saved[1].status, SaveStatus::Saved);
    assert!(saved[1].previous.is_none());
    let [warning] = saved[1].warnings.as_slice() else {
        panic!("{:?}", saved[1].warnings);
    };
    for expected in [
        "API root `wire_v2::Api` has no earlier snapshot",
        "on line 2 with a root trait `Api`",
        "test_api_0_1_0.ron (whole crate, w1.0, traits `Api`)",
    ] {
        assert!(
            warning.contains(expected),
            "'{expected}' not in:\n{warning}"
        );
    }
}

/// A root renamed as it moved into its module matches no earlier snapshot by name: it is saved with a warning naming
/// what was looked for and what is there, not silently unchecked (EVO-38).
#[test]
fn renamed_root_is_saved_with_a_warning() {
    let c = TestCrate::new("renamed_root");
    c.write(
        "0.1.0",
        "#[ww_api_root(wire = \"w1.0\")]\npub trait Api {\n    fn led_on();\n}\n",
    )
    .save();
    let line_1 = "#[ww_api_root]\npub trait Led {\n    fn led_on();\n}\n";
    c.write_roots("0.2.0", &[("wire_v1::Led", "w1.0")], &[("wire_v1", line_1)]);
    let saved = save_one(c.path(), false);
    assert_eq!(saved.status, SaveStatus::Saved);
    assert!(saved.previous.is_none());
    let [warning] = saved.warnings.as_slice() else {
        panic!("{:?}", saved.warnings);
    };
    for expected in [
        "API root `wire_v1::Led` has no earlier snapshot, so it is saved unchecked",
        "test_api_*.wire_v1.Led@w*.ron",
        "with a root trait `Led`",
        "traits `Api`",
    ] {
        assert!(
            warning.contains(expected),
            "'{expected}' not in:\n{warning}"
        );
    }
    // still unchecked while 0.2.0 is worked on; the next version is checked against its own file, and isn't warned
    assert_eq!(save_one(c.path(), false).warnings.len(), 1);
    let line_1 = "/// Docs.\n#[ww_api_root]\npub trait Led {\n    fn led_on();\n}\n";
    c.write_roots("0.2.1", &[("wire_v1::Led", "w1.0")], &[("wire_v1", line_1)]);
    let saved = save_one(c.path(), false);
    assert!(saved.warnings.is_empty(), "{:?}", saved.warnings);
    assert!(saved.previous.is_some());
}

/// w0, the design line (EVO-41): no snapshot is saved or required, a difference against one is information, and
/// the move to w1.0 saves the first snapshot.
#[test]
fn design_line_keeps_no_snapshots() {
    let design = API.replace("wire = \"w1.0\"", "wire = \"w0.3\"");
    let c = TestCrate::new("design_line");
    c.write("0.1.0", &design);
    let saved = save_one(c.path(), false);
    assert_eq!(saved.status, SaveStatus::DesignLine);
    assert!(
        saved.warnings.is_empty(),
        "a test crate: {:?}",
        saved.warnings
    );
    assert!(!c.path().join(SNAPSHOTS_DIR).exists());

    // strict and require_current pass: none is missing
    for outcome in [
        Check::new(c.path()).strict(true).run().unwrap(),
        Check::new(c.path()).require_current(true).run().unwrap(),
    ] {
        let Outcome::NoSnapshot(n) = &outcome else {
            panic!("{outcome:?}")
        };
        assert!(n.design_line);
        has_all(
            &outcome.verdict().unwrap(),
            &["w0 is the design line", "breaks are accepted"],
        );
    }
    Check::new(c.path()).require_current(true).assert();

    // a break against a snapshot (one kept from before, or `--against`) is information, no bump of either kind
    c.save();
    let snapshot = c.snapshot("test_api_0_1_0.ron");
    c.write("0.1.0", &design.replace("    fn led_off();\n", ""));
    for check in [
        Check::new(c.path()).strict(true),
        Check::new(c.path()).require_current(true),
        Check::new(c.path()).against(&snapshot),
    ] {
        let outcome = check.run().unwrap();
        let Outcome::Compared(comparison) = &outcome else {
            panic!("{outcome:?}")
        };
        assert!(comparison.design_line);
        assert_eq!(comparison.report.change, Change::Breaking);
        assert!(comparison.report.check_wire().is_ok());
        assert!(
            comparison
                .report
                .required()
                .contains("wire w0.3 (the design line: no wire bump)"),
            "{}",
            comparison.report.required()
        );
        has_all(
            &outcome.verdict().unwrap(),
            &[
                "w0.3 is the design line",
                "the differences above are information",
            ],
        );
    }
    std::fs::remove_dir_all(c.path().join(SNAPSHOTS_DIR)).unwrap();

    // the first release: w1.0, and the first snapshot
    c.write(
        "0.2.0",
        &design.replace("wire = \"w0.3\"", "wire = \"w1.0\""),
    );
    assert_eq!(save_one(c.path(), false).status, SaveStatus::Saved);
    assert!(c.snapshot("test_api_0_2_0.ron").exists());
    assert_eq!(check_current(c.path()).1.unwrap(), "no changes");
}

/// A crate that can be published with its root on w0 (EVO-41): `ww api check` and `ww api save` warn, the verdict
/// passes.
#[test]
fn design_line_in_a_publishable_crate_warns() {
    let c = TestCrate::new("design_line_publishable");
    std::fs::write(
        c.path().join("Cargo.toml"),
        "[package]\nname = \"test_api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [package.metadata.wire_weaver]\nwire = \"w0.0\"\n\n[dependencies]\n",
    )
    .unwrap();
    std::fs::write(
        c.path().join("src/lib.rs"),
        "use wire_weaver::prelude::*;\n#[ww_api_root]\npub trait Api {\n    fn led_on();\n}\n",
    )
    .unwrap();
    let outcome = Check::new(c.path()).strict(true).run().unwrap();
    assert!(outcome.verdict().is_ok());
    let [warning] = outcome.warnings() else {
        panic!("{:?}", outcome.warnings())
    };
    has_all(
        warning,
        &[
            "test_api can be published",
            "w0.0, the design line",
            "move it to w1.0 before publishing",
        ],
    );
    assert!(
        outcome
            .to_string()
            .contains("warning: test_api can be published")
    );
    let saved = save_one(c.path(), false);
    assert_eq!(saved.status, SaveStatus::DesignLine);
    assert_eq!(saved.warnings.len(), 1, "{:?}", saved.warnings);
}

/// A w0 root next to a released one in one crate (EVO-41): only the released root gets a snapshot.
#[test]
fn design_line_root_gets_no_snapshot_of_its_own() {
    let c = TestCrate::new("design_line_root");
    let roots = [("wire_v1::Api", "w1.0"), ("wire_v2::Api", "w0.0")];
    let modules = [("common", COMMON), ("wire_v1", LINE_1), ("wire_v2", LINE_2)];
    c.write_roots("0.1.0", &roots, &modules);
    assert_eq!(
        saved_roots(c.path()),
        [
            (
                "wire_v1::Api".into(),
                "test_api_0_1_0.wire_v1.Api@w1.0.ron".into(),
                SaveStatus::Saved
            ),
            (
                "wire_v2::Api".into(),
                "test_api_0_1_0.wire_v2.Api@w0.0.ron".into(),
                SaveStatus::DesignLine
            ),
        ]
    );
    assert!(!c.snapshot("test_api_0_1_0.wire_v2.Api@w0.0.ron").exists());
    // checked per root (EVO-39): the w0 root needs no snapshot, and breaks on it are information
    let (outcome, verdict) = check_current(c.path());
    let ok = verdict.unwrap_or_else(|e| panic!("{outcome}\n{e}"));
    has_all(
        &ok,
        &[
            "`wire_v1::Api`: no changes",
            "`wire_v2::Api`: w0 is the design line",
        ],
    );
    let checked = roots_of(&outcome);
    assert!(matches!(&checked.roots[1].outcome, Outcome::NoSnapshot(n) if n.design_line));
    let line_2b = LINE_2.replace("    fn read_once(average: u8) -> Code;\n", "");
    let modules = [
        ("common", COMMON),
        ("wire_v1", LINE_1),
        ("wire_v2", &line_2b),
    ];
    c.write_roots("0.1.0", &roots, &modules);
    assert!(check_current(c.path()).1.is_ok());
}

/// The roots of a crate with several, as [Check::run] found them.
fn roots_of(outcome: &Outcome) -> &RootsOutcome {
    match outcome {
        Outcome::Roots(roots) => roots,
        other => panic!("not checked per root: {other}"),
    }
}

/// The modules of [checker_per_root]'s crate, line 1's and line 2's root with `extra` added to the shared `Channel`.
fn per_root_modules(common_extra: &str) -> Vec<(&'static str, String)> {
    let common = COMMON.replace(
        "    fn read() -> Code;\n",
        &format!("    fn read() -> Code;\n{common_extra}"),
    );
    vec![
        ("common", common),
        ("wire_v1", LINE_1.to_string()),
        ("wire_v2", LINE_2.to_string()),
    ]
}

fn write_per_root(c: &TestCrate, version: &str, wires: [&str; 2], modules: &[(&str, String)]) {
    let roots = [("wire_v1::Api", wires[0]), ("wire_v2::Api", wires[1])];
    let modules: Vec<(&str, &str)> = modules.iter().map(|(n, s)| (*n, s.as_str())).collect();
    c.write_roots(version, &roots, &modules);
}

/// A crate with several roots is checked per root (EVO-39): each against its own latest snapshot, `#[since]` and the
/// wire version on each line, a shared type that changed named with every root it changed on, the crate version
/// the largest any root needs.
#[test]
fn checker_per_root() {
    let c = TestCrate::new("checker_per_root");
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &per_root_modules(""));
    Check::new(c.path()).save(false).unwrap();
    let (outcome, verdict) = check_current(c.path());
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    assert_eq!(roots_of(&outcome).roots.len(), 2);

    // a method of the shared sub-trait, with a revision per line: a new revision of each, a compatible crate bump
    let reset = "    #[since = \"w1.1, w2.1\"]\n    fn reset();\n";
    write_per_root(&c, "0.1.1", ["w1.1", "w2.0"], &per_root_modules(reset));
    let (outcome, verdict) = check(c.path(), true);
    let err = verdict.unwrap_err();
    has_all(
        &err,
        &[
            "API root `wire_v2::Api` (w2.0): test_api wire w2.0 -> w2.0 has",
            "Channel::reset has #[since = \"w2.1\"], newer than the API's wire version w2.0",
            "`\"wire_v2::Api\"` of `[package.metadata.wire_weaver.roots]`",
        ],
    );
    assert!(!err.contains("wire_v1::Api"), "{err}");
    let roots = roots_of(&outcome);
    assert_eq!(roots.shared.len(), 1, "{outcome}");
    assert_eq!(
        roots.shared[0].to_string(),
        "shared `common::Channel` changed on `wire_v1::Api` (w1.1), `wire_v2::Api` (w2.0)"
    );
    // the `#[since]` of each line is checked against that line's revision
    let early = "    #[since = \"w1.0, w2.1\"]\n    fn reset();\n";
    write_per_root(&c, "0.1.1", ["w1.1", "w2.1"], &per_root_modules(early));
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{verdict:?}");
    let text = outcome.to_string();
    let line_2 = text.find("API root `wire_v2::Api`").unwrap();
    let warning = text
        .find("warning: Channel::reset is added with #[since = \"w1.0\"], expected #[since = \"w1.1\"]")
        .unwrap_or_else(|| panic!("{text}"));
    assert!(warning < line_2, "under line 1:\n{text}");
    assert!(!text[line_2..].contains("warning"), "{text}");
    write_per_root(&c, "0.1.1", ["w1.1", "w2.1"], &per_root_modules(reset));
    let (outcome, verdict) = check(c.path(), true);
    assert_eq!(
        verdict.unwrap(),
        "`wire_v1::Api`: version is bumped enough; `wire_v2::Api`: version is bumped enough",
        "{outcome}"
    );
    assert_eq!(
        roots_of(&outcome)
            .minimal_version()
            .map(|v| wire_weaver_evolution::snapshot_files::triplet(&v)),
        Some("0.1.1".to_string())
    );

    // a shared type changed on the wire: both lines need a new one, the crate a breaking bump, the largest
    let wider = per_root_modules(reset)
        .into_iter()
        .map(|(n, s)| {
            (
                n,
                s.replace("pub struct Code(pub u16);", "pub struct Code(pub u32);"),
            )
        })
        .collect::<Vec<_>>();
    write_per_root(&c, "0.1.2", ["w1.1", "w2.1"], &wider);
    let (outcome, verdict) = check(c.path(), true);
    let err = verdict.unwrap_err();
    has_all(
        &err,
        &[
            "API root `wire_v1::Api` (w1.1)",
            "API root `wire_v2::Api` (w2.1)",
            "0.2.0 or later",
            "w2.0 or later",
            "w3.0 or later",
        ],
    );
    let text = outcome.to_string();
    has_all(
        &text,
        &[
            "shared `common::Code` changed on `wire_v1::Api` (w1.1), `wire_v2::Api` (w2.1)",
            "crate: 0.2.0 or later, the largest its roots need",
        ],
    );
}

/// Two roots that each reach a trait of one name, line 1 its own `wire_v1::Channel` and line 2 `common::Channel`,
/// don't share it: a change to `common::Channel` is line 2's only, and nothing is named shared (EVO-42, found when
/// the bench's API moved into one crate). The API model names a trait by crate and name, so the walker notes the
/// module of each.
#[test]
fn same_name_in_two_modules_is_not_shared() {
    let c = TestCrate::new("same_name_in_two_modules");
    let modules = |common_extra: &str, own_extra: &str| {
        let own_channel = LINE_1.replace("super::common::Channel", "Channel")
            + &format!(
                "\n#[ww_trait]\npub trait Channel {{\n    fn read() -> Code;\n{own_extra}}}\n"
            );
        let mut m = per_root_modules(common_extra);
        m[1].1 = own_channel;
        m
    };
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &modules("", ""));
    Check::new(c.path()).save(false).unwrap();

    // `common::Channel` changed: line 2's only
    let reset = "    #[since = \"w2.1\"]\n    fn reset();\n";
    write_per_root(&c, "0.1.1", ["w1.0", "w2.1"], &modules(reset, ""));
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    assert!(roots_of(&outcome).shared.is_empty(), "{outcome}");
    let text = outcome.to_string();
    let line_2 = text.find("API root `wire_v2::Api`").unwrap();
    assert!(
        !text[..line_2].contains("reset"),
        "line 1 has its own Channel:\n{text}"
    );
    Check::new(c.path()).save(false).unwrap();

    // and the other way round: `wire_v1::Channel` changed, line 1's only
    let calibrate = "    #[since = \"w1.1\"]\n    fn calibrate();\n";
    write_per_root(&c, "0.1.2", ["w1.1", "w2.1"], &modules(reset, calibrate));
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    assert!(roots_of(&outcome).shared.is_empty(), "{outcome}");
    let text = outcome.to_string();
    let line_2 = text.find("API root `wire_v2::Api`").unwrap();
    assert!(text[..line_2].contains("calibrate"), "{text}");
    assert!(
        !text[line_2..].contains("calibrate"),
        "line 2 reaches common::Channel only:\n{text}"
    );
}

/// Two types of one name in two modules, both reached by one root (API-46), are two types to the checker too: a
/// change to `other::Point` is reported as that type's, `geometry::Point` is left alone (EVO-42).
#[test]
fn two_types_of_one_name_in_one_root() {
    for (changed, unchanged) in [("other", "geometry"), ("geometry", "other")] {
        let c = TestCrate::new(&format!("two_types_of_one_name_{changed}"));
        let write = |version: &str, wire: &str, ty: &str| {
            let point = |module: &str, field: &str| {
                let ty = if module == changed { ty } else { "u8" };
                format!("#[derive_shrink_wrap]\npub struct Point {{\n    pub {field}: {ty},\n}}\n")
            };
            let root = "#[ww_api_root]\npub trait Api {\n    fn flat() -> super::geometry::Point;\n    \
                        fn high() -> super::other::Point;\n}\n";
            let (geometry, other) = (point("geometry", "x"), point("other", "z"));
            let modules = [
                ("geometry", geometry.as_str()),
                ("other", other.as_str()),
                ("wire_v1", root),
            ];
            c.write_roots(version, &[("wire_v1::Api", wire)], &modules);
        };
        write("0.1.0", "w1.0", "u8");
        Check::new(c.path()).save(false).unwrap();
        let (outcome, verdict) = check(c.path(), true);
        assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");

        write("0.2.0", "w2.0", "u16");
        let (outcome, _) = check(c.path(), false);
        let text = outcome.to_string();
        assert!(
            text.contains(&format!("type test_api::{changed}::Point changed")),
            "{text}"
        );
        assert!(!text.contains(&format!("{unchanged}::Point")), "{text}");
    }
}

/// A root that was saved before and that the crate no longer lists is a line leaving the crate: a breaking crate
/// bump, recorded by [Check::save] so the next version isn't asked again (EVO-39).
#[test]
fn removed_root_needs_a_breaking_bump() {
    let c = TestCrate::new("removed_root");
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &per_root_modules(""));
    Check::new(c.path()).save(false).unwrap();
    let only_2 = |version: &str| {
        c.write_roots(
            version,
            &[("wire_v2::Api", "w2.0")],
            &[("common", COMMON), ("wire_v2", LINE_2)],
        );
    };
    only_2("0.1.1");
    let err = check(c.path(), false).1.unwrap_err();
    has_all(
        &err,
        &[
            "API root `wire_v1::Api` is removed (its last snapshot is",
            "test_api_0_1_0.wire_v1.Api@w1.0.ron",
            "test_api 0.1.0 -> 0.2.0 or later",
        ],
    );
    let err = Check::new(c.path()).save(false).unwrap_err();
    assert!(err.contains("Not saved: bump `version`"), "{err}");

    only_2("0.2.0");
    let (outcome, verdict) = check(c.path(), false);
    assert_eq!(
        verdict.unwrap(),
        "`wire_v2::Api`: no changes; `wire_v1::Api`: removed, version is bumped enough",
        "{outcome}"
    );
    // the evolution test wants it recorded
    let err = check_current(c.path()).1.unwrap_err();
    assert!(err.contains("the removal is not recorded"), "{err}");
    let saved = Check::new(c.path()).save(false).unwrap();
    let removed = saved
        .iter()
        .find(|s| s.status == SaveStatus::Removed)
        .unwrap_or_else(|| panic!("{saved:?}"));
    assert_eq!(removed.root.as_deref(), Some("wire_v1::Api"));
    let record = c.snapshot("test_api_0_2_0.wire_v1.Api@w1.0.removed");
    assert_eq!(removed.path, record);
    let text = std::fs::read_to_string(&record).unwrap();
    assert!(text.contains("API root wire_v1::Api removed"), "{text}");
    let (outcome, verdict) = check_current(c.path());
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    // recorded once: the next version is not asked again, and saving again writes nothing
    only_2("0.2.1");
    assert!(check(c.path(), false).1.is_ok());
    let saved = Check::new(c.path()).save(false).unwrap();
    assert!(
        saved.iter().all(|s| s.status == SaveStatus::UpToDate),
        "{saved:?}"
    );
}

/// A root whose latest snapshot is older than the crate version, which saved other roots: that snapshot stands for
/// the crate's version, so a change to the root after the save needs a new version, as for a whole crate (EVO-39).
#[test]
fn unchanged_roots_snapshot_stands_for_the_saved_version() {
    let c = TestCrate::new("stands_for");
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &per_root_modules(""));
    Check::new(c.path()).save(false).unwrap();
    // 0.1.1 changes only line 2, and is saved
    let mut modules = per_root_modules("");
    modules[2].1 = LINE_2.replace(
        "Channel);\n",
        "Channel);\n    #[since = \"w2.1\"]\n    fn stop();\n",
    );
    write_per_root(&c, "0.1.1", ["w1.0", "w2.1"], &modules);
    Check::new(c.path()).save(false).unwrap();
    assert!(check_current(c.path()).1.is_ok());
    // then line 1 gets a doc comment without a new version
    modules[1].1 = LINE_1.replace("#[ww_api_root]", "/// Line 1.\n#[ww_api_root]");
    write_per_root(&c, "0.1.1", ["w1.0", "w2.1"], &modules);
    let err = check(c.path(), false).1.unwrap_err();
    has_all(
        &err,
        &[
            "API root `wire_v1::Api` (w1.0)",
            "test_api 0.1.1 has snapshots, saved with this root unchanged since 0.1.0",
            "test_api_0_1_0.wire_v1.Api@w1.0.ron",
        ],
    );
    let err = check_current(c.path()).1.unwrap_err();
    has_all(
        &err,
        &[
            "The source differs from",
            "(unchanged since 0.1.0, so no file of its own)",
        ],
    );
    // a root's snapshot file is compared with the previous one of the same root, `--against` takes a root's file
    let v2 = c.snapshot("test_api_0_1_1.wire_v2.Api@w2.1.ron");
    let outcome = Check::new(&v2).run().unwrap();
    let Outcome::Compared(compared) = &outcome else {
        panic!("{outcome}");
    };
    assert_eq!(
        compared.old_path,
        c.snapshot("test_api_0_1_0.wire_v2.Api@w2.0.ron")
    );
    assert_eq!(compared.report.change, Change::Compatible);
    let outcome = Check::new(c.path()).against(&v2).run().unwrap();
    let roots = roots_of(&outcome);
    assert_eq!(roots.roots.len(), 1);
    assert_eq!(roots.roots[0].root, "wire_v2::Api");
    let err = Check::new(c.path())
        .against(
            c.snapshot("test_api_0_1_0.wire_v1.Api@w1.0.ron")
                .with_extension("x"),
        )
        .run()
        .unwrap_err();
    assert!(
        err.contains("compare one of them with a snapshot of that root"),
        "{err}"
    );
}

/// A root shipped by 0.1.0, changed and saved by 0.1.1, then removed while 0.1.1 is worked on (EVO-39): `run` and
/// `save` agree that the removal needs a breaking bump of 0.1.0, the last version that shipped it, and once recorded
/// the removal is recognised, so saving again writes nothing and the evolution test passes.
#[test]
fn removal_of_a_root_saved_by_the_current_version() {
    let c = TestCrate::new("removed_after_save");
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &per_root_modules(""));
    Check::new(c.path()).save(false).unwrap();
    let mut modules = per_root_modules("");
    modules[1].1 = LINE_1.replace("#[ww_api_root]", "/// Line 1.\n#[ww_api_root]");
    write_per_root(&c, "0.1.1", ["w1.0", "w2.0"], &modules);
    Check::new(c.path()).save(false).unwrap();
    assert!(c.snapshot("test_api_0_1_1.wire_v1.Api@w1.0.ron").exists());
    let only_2 = |version: &str| {
        c.write_roots(
            version,
            &[("wire_v2::Api", "w2.0")],
            &[("common", COMMON), ("wire_v2", LINE_2)],
        );
    };
    only_2("0.1.1");
    let err = check(c.path(), false).1.unwrap_err();
    assert!(err.contains("test_api 0.1.0 -> 0.2.0 or later"), "{err}");
    let err = Check::new(c.path()).save(false).unwrap_err();
    assert!(err.contains("test_api 0.1.0 -> 0.2.0 or later"), "{err}");
    assert!(
        !c.snapshot("test_api_0_1_1.wire_v1.Api@w1.0.removed")
            .exists()
    );

    only_2("0.2.0");
    assert!(check(c.path(), false).1.is_ok());
    let saved = Check::new(c.path()).save(false).unwrap();
    assert!(
        saved.iter().any(|s| s.status == SaveStatus::Removed),
        "{saved:?}"
    );
    let saved = Check::new(c.path()).save(false).unwrap();
    assert!(
        saved.iter().all(|s| s.status == SaveStatus::UpToDate),
        "{saved:?}"
    );
    let (outcome, verdict) = check_current(c.path());
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
}

/// A root that only ever had a snapshot of the version being worked on was never shipped: removing it needs no bump.
#[test]
fn removal_of_a_root_never_shipped_needs_no_bump() {
    let c = TestCrate::new("removed_never_shipped");
    c.write_roots(
        "0.1.0",
        &[("wire_v2::Api", "w2.0")],
        &[("common", COMMON), ("wire_v2", LINE_2)],
    );
    Check::new(c.path()).save(false).unwrap();
    write_per_root(&c, "0.1.1", ["w1.0", "w2.0"], &per_root_modules(""));
    Check::new(c.path()).save(false).unwrap();
    c.write_roots(
        "0.1.1",
        &[("wire_v2::Api", "w2.0")],
        &[("common", COMMON), ("wire_v2", LINE_2)],
    );
    assert!(check(c.path(), false).1.is_ok());
    let saved = Check::new(c.path()).save(false).unwrap();
    assert!(
        saved.iter().any(|s| s.status == SaveStatus::Removed),
        "{saved:?}"
    );
    assert!(check_current(c.path()).1.is_ok());
}

/// A check keeps its loads in a cache that sees edits: it stays `Send` and `Sync`, checks given one cache share it,
/// also on other threads, and runs again after an edit see the edit (EVO-39, EVO-50).
#[test]
fn a_check_sees_the_source_as_it_is_now() {
    fn send_sync<T: Send + Sync>(_: &T) {}
    let c = TestCrate::new("check_sees_edits");
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &per_root_modules(""));
    let cache = CrateCache::default();
    Check::new(c.path())
        .cache(cache.clone())
        .save(false)
        .unwrap();
    let check = Check::new(c.path()).strict(true).cache(cache);
    send_sync(&check);
    assert!(check.run().unwrap().verdict().is_ok());
    let again = check.clone();
    let mut modules = per_root_modules("");
    modules[1].1 = LINE_1.replace("    fn read_once() -> Code;\n", "");
    write_per_root(&c, "0.1.0", ["w1.0", "w2.0"], &modules);
    let err = std::thread::spawn(move || again.run().unwrap().verdict().unwrap_err())
        .join()
        .unwrap();
    assert!(err.contains("API root `wire_v1::Api`"), "{err}");
    assert!(check.run().unwrap().verdict().is_err());
}

/// A shared type a w0 root reaches too (EVO-41): listed with the w0 root, which needs no bump of its own.
#[test]
fn shared_type_reached_by_a_design_line_root() {
    let c = TestCrate::new("shared_with_w0");
    let modules = per_root_modules("");
    write_per_root(&c, "0.1.0", ["w1.0", "w0.0"], &modules);
    Check::new(c.path()).save(false).unwrap();
    let wider: Vec<_> = modules
        .into_iter()
        .map(|(n, s)| {
            (
                n,
                s.replace(
                    "pub struct Code(pub u16);",
                    "pub struct Code(pub u16, pub u8);",
                ),
            )
        })
        .collect();
    write_per_root(&c, "0.2.0", ["w2.0", "w0.1"], &wider);
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    let roots = roots_of(&outcome);
    assert_eq!(
        roots
            .shared
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        [
            "shared `common::Code` changed on `wire_v1::Api` (w2.0), also reached by `wire_v2::Api` (w0.1)"
        ]
    );
    assert_eq!(
        roots
            .minimal_version()
            .map(|v| wire_weaver_evolution::snapshot_files::triplet(&v)),
        Some("0.2.0".to_string())
    );
}

/// Line 2 of an API, and line 3 with `self_test` moved into a sub-level and `uptime` replaced there by a wider one.
const MOVE_W2: &str = r#"
#[ww_api_root(wire = "w2.1")]
pub trait Api {
    fn configure(rate: u16);
    property!(ro uptime: u32);
    fn self_test() -> bool;
}
"#;

const MOVE_W3: &str = r#"
#[ww_api_root(wire = "w3.0")]
pub trait Api {
    fn configure(rate: u16);
    ww_impl!(health: Health);
}

#[ww_trait]
pub trait Health {
    property!(ro uptime: u64);
    #[moved_from(w2.1, Api::self_test)]
    fn self_test() -> bool;
}
"#;

/// `#[moved_from]` (EVO-40) read from the source by [Check]: a crate's new line against its previous line's snapshot
/// reports the move and proposes nothing for it, `ww api save` refuses an attribute that doesn't hold.
#[test]
fn moved_from_is_read_from_the_source() {
    let c = TestCrate::new("moved_from_is_read_from_the_source");
    c.write("0.1.0", MOVE_W2).save();
    c.write("0.2.0", MOVE_W3);
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    let text = outcome.to_string();
    has_all(
        &text,
        &[
            "breaking: Api::self_test moved to Health::self_test (#[moved_from], the same signature)",
            "rules.md#moving-a-resource-to-another-place",
            "breaking: Api::uptime removed",
            "proposal: Health::uptime is named like Api::uptime of w2.1, removed, but its signature differs",
        ],
    );
    assert!(!text.contains("Api::self_test removed"), "{text}");

    let same_line = MOVE_W3.replace("(w2.1, Api::self_test)", "(w3.0, Api::self_test)");
    c.write("0.2.0", &same_line);
    let err = check(c.path(), true).1.unwrap_err();
    has_all(
        &err,
        &[
            "#[moved_from(w3.0, Api::self_test)] on Health::self_test: the API is at w3.0: within a line",
        ],
    );
    let err = Check::new(c.path()).save(false).unwrap_err();
    assert!(
        err.contains("within a line a resource is its position"),
        "{err}"
    );
}

/// In a crate with several roots, a root that begins a new line has no snapshot of its own line: it is compared with
/// the previous line's root of the same name for its moves (EVO-40), and fails only on a `#[moved_from]` that doesn't
/// hold.
#[test]
fn moved_from_of_a_new_root() {
    let c = TestCrate::new("moved_from_of_a_new_root");
    c.write_roots(
        "0.1.0",
        &[("wire_v2::Api", "w2.1")],
        &[("wire_v2", MOVE_W2)],
    );
    Check::new(c.path()).save(false).unwrap();
    let roots = [("wire_v2::Api", "w2.1"), ("wire_v3::Api", "w3.0")];
    c.write_roots(
        "0.2.0",
        &roots,
        &[("wire_v2", MOVE_W2), ("wire_v3", MOVE_W3)],
    );
    let (outcome, verdict) = check(c.path(), false);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    let new_line = &roots_of(&outcome).roots[1];
    assert_eq!(new_line.root, "wire_v3::Api");
    has_all(
        &new_line.outcome.to_string(),
        &[
            "no snapshot to compare with",
            "the line before: ",
            "test_api_0_1_0.wire_v2.Api@w2.1.ron",
            "moved: Api::self_test moved to Health::self_test",
            "proposal: Health::uptime is named like Api::uptime of w2.1",
        ],
    );

    let nothing = MOVE_W3.replace("Api::self_test)", "Api::self_check)");
    c.write_roots(
        "0.2.0",
        &roots,
        &[("wire_v2", MOVE_W2), ("wire_v3", &nothing)],
    );
    let err = check(c.path(), false).1.unwrap_err();
    has_all(
        &err,
        &[
            "#[moved_from(w2.1, Api::self_check)] on Health::self_test: w2.1, compared with, has no Api::self_check",
            "test_api_0_1_0.wire_v2.Api@w2.1.ron",
        ],
    );
}

/// Line 1, 2 and 3 of a chain: `check` on line 1 becomes `self_test` on line 2, then moves into `Health` on line 3,
/// which keeps both links.
const CHAIN_W1: &str = r#"
#[ww_api_root(wire = "w1.0")]
pub trait Api {
    fn configure(rate: u16);
    fn check() -> bool;
}
"#;

const CHAIN_W2: &str = r#"
#[ww_api_root(wire = "w2.1")]
pub trait Api {
    fn configure(rate: u16);
    #[moved_from(w1.0, Api::check)]
    fn self_test() -> bool;
}
"#;

const CHAIN_W3: &str = r#"
#[ww_api_root(wire = "w3.0")]
pub trait Api {
    fn configure(rate: u16);
    ww_impl!(health: Health);
}

#[ww_trait]
pub trait Health {
    #[moved_from(w2.1, Api::self_test)]
    #[moved_from(w1.0, Api::check)]
    fn self_test() -> bool;
}
"#;

/// A chain over two breaks (EVO-40): the link to the line compared with is checked by the comparison, the older one
/// against the snapshot of its own line, on the first revision of the new line and on every later one; a link that
/// doesn't hold fails either way, one without a snapshot of its line is a warning.
#[test]
fn moved_from_chain_over_two_breaks() {
    let c = TestCrate::new("moved_from_chain_over_two_breaks");
    c.write("0.1.0", CHAIN_W1);
    save_one(c.path(), false);
    c.write("0.2.0", CHAIN_W2);
    assert_eq!(save_one(c.path(), false).status, SaveStatus::Saved);
    c.write("0.3.0", CHAIN_W3);
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    assert!(
        outcome
            .to_string()
            .contains("Api::self_test moved to Health::self_test"),
        "{outcome}"
    );
    assert_eq!(save_one(c.path(), false).status, SaveStatus::Saved);

    // a later revision of line 3: both links are verified against their lines' snapshots
    c.write("0.3.1", &CHAIN_W3.replace("w3.0", "w3.1"));
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    for (from, to, expected) in [
        (
            "(w1.0, Api::check)",
            "(w1.0, Api::nope)",
            "w1.0, compared with, has no Api::nope (as in ",
        ),
        (
            "(w2.1, Api::self_test)",
            "(w2.9, Api::self_test)",
            "w2.9 is newer than w2.1",
        ),
    ] {
        c.write("0.3.1", &CHAIN_W3.replace("w3.0", "w3.1").replace(from, to));
        let err = check(c.path(), true).1.unwrap_err();
        assert!(err.contains(expected), "{expected} not in {err}");
    }
    // a line without its snapshot can't be checked: said, not dropped
    std::fs::remove_file(c.snapshot("test_api_0_1_0.ron")).unwrap();
    c.write("0.3.1", &CHAIN_W3.replace("w3.0", "w3.1"));
    let (outcome, verdict) = check(c.path(), true);
    assert!(verdict.is_ok(), "{outcome}\n{verdict:?}");
    assert!(
        outcome
            .to_string()
            .contains("#[moved_from(w1.0, Api::check)] on Health::self_test: not verified, ")
            && outcome.to_string().contains("has no snapshot of line 1"),
        "{outcome}"
    );
}

/// A `#[moved_from]` on a trait outside the root's tree is still checked: a whole-crate snapshot has every
/// `#[ww_trait]` of the crate, so nothing is skipped for not being reached from the root.
#[test]
fn moved_from_on_a_trait_outside_the_root_tree() {
    let c = TestCrate::new("moved_from_on_a_trait_outside_the_root_tree");
    c.write("0.1.0", MOVE_W2).save();
    let unused = format!(
        "{MOVE_W3}\n#[ww_trait]\npub trait Unused {{\n    #[moved_from(w2.1, Api::configure)]\n    fn configure(rate: u16);\n}}\n"
    );
    c.write("0.2.0", &unused);
    let err = check(c.path(), true).1.unwrap_err();
    assert!(
        err.contains(
            "#[moved_from(w2.1, Api::configure)] on Unused::configure: Api::configure is still there on the new \
             version"
        ),
        "{err}"
    );
}
