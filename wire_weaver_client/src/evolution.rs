//! Evolution checker: compares a crate's traits and types with a snapshot of an earlier version of the same crate
//! (saved by `ww api save`), and tells which version position has to be bumped, following
//! [evolution rules](https://github.com/vhrdtech/wire_weaver/blob/master/docs/evolution/rules.md).
//!
//! * Breaking position (minor before 1.0, major after): a trait, resource or type is removed or renamed, a resource or
//!   type can't be used with the other version anymore in either direction (e.g. a field without `#[default]` is added,
//!   an enum gains a variant, an argument changes type), or a dependency crate is upgraded to an incompatible version.
//!   Renames don't change the wire, but they break Rust code, which is a SemVer break as well.
//! * Compatible position (patch before 1.0, minor after): anything else that changes, including doc comments only,
//!   as a released version's snapshot never changes.
//!
//! Resources are matched by id inside each trait, traits and types by name.
//!
//! [diff] lists every difference instead, doc comments included, without classifying them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use wire_weaver::shrink_wrap::{ElementSize, SerializeShrinkWrapOwned};
use ww_self::errors::fmt_error_list;
use ww_self::visit_mut::VisitMut;
use ww_self::{
    ApiBundleOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned, ErrorVariantOwned,
    FieldsOwned, TypeLocationOwned, TypeOwned,
};
use ww_version::{FullVersionOwned, VersionOwned, VersionTriplet};

use crate::compat::{Bounds, TypeCheck, compare_items, field_name, fields};
use crate::layout;

mod diff;
pub use diff::{Difference, diff};

const OLD: &str = "old version";
const NEW: &str = "new version";

/// Kind of change between two versions of a crate, from the least to the most severe.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    /// Traits and types are exactly the same, docs included.
    None,
    /// Only doc comments changed.
    DocsOnly,
    /// Wire and Rust compatible changes, e.g. new resources, new fields with `#[default]`, new types.
    Compatible,
    /// Old and new versions can't be used together.
    Breaking,
}

/// Version position that has to be bumped.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    None,
    /// Patch before 1.0, minor after.
    Compatible,
    /// Minor before 1.0, major after.
    Breaking,
}

impl Change {
    pub fn required_bump(&self) -> Bump {
        match self {
            Change::None => Bump::None,
            Change::DocsOnly | Change::Compatible => Bump::Compatible,
            Change::Breaking => Bump::Breaking,
        }
    }
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Change::None => "no changes",
            Change::DocsOnly => "doc comments only",
            Change::Compatible => "compatible changes",
            Change::Breaking => "breaking changes",
        })
    }
}

impl fmt::Display for Bump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Bump::None => "no bump",
            Bump::Compatible => "compatible position (patch before 1.0, minor after)",
            Bump::Breaking => "breaking position (minor before 1.0, major after)",
        })
    }
}

/// Result of comparing two versions of a crate, see [compare].
#[derive(Debug)]
pub struct Report {
    pub old: FullVersionOwned,
    pub new: FullVersionOwned,
    pub change: Change,
    /// Why the change is breaking, empty if it isn't.
    pub breaking: Vec<String>,
    /// Additions and other compatible changes found.
    pub compatible: Vec<String>,
    /// Not affecting the verdict, e.g. an added resource without `#[since]`.
    pub warnings: Vec<String>,
}

impl Report {
    /// Smallest version the new one has to be for the changes found.
    pub fn minimal_version(&self) -> VersionOwned {
        suggest(&self.old.version, self.change.required_bump())
    }

    /// Checks that the new version is bumped enough for the changes found. Otherwise, returns an error with the
    /// smallest sufficient version.
    pub fn check_version(&self) -> Result<(), String> {
        let (old, new) = (&self.old.version, &self.new.version);
        let required = self.change.required_bump();
        if triplet(new) < triplet(old) {
            return Err(format!(
                "new version {} is older than {}",
                display(new),
                display(old)
            ));
        }
        if bump(old, new) >= required {
            return Ok(());
        }
        Err(format!(
            "{} {} -> {} has {}, which requires a bump of the {}: {} or later",
            self.new.crate_id,
            display(old),
            display(new),
            self.change,
            required,
            display(&self.minimal_version())
        ))
    }
}

/// Compare an old version of a crate with a new one. Both are crate bundles, as loaded by
/// `wire_weaver_core::load_crate` and saved by `ww api save`.
pub fn compare(old: &ApiBundleOwned, new: &ApiBundleOwned) -> Result<Report, String> {
    let (Some(old_version), Some(new_version)) = (old.ext_crates.first(), new.ext_crates.first())
    else {
        return Err("not a crate snapshot, ext_crates[0] must be the crate itself".into());
    };
    if old_version.crate_id != new_version.crate_id {
        return Err(format!(
            "comparing different crates: {} and {}",
            old_version.crate_id, new_version.crate_id
        ));
    }
    // own traits and types are then from the same crate version on both sides, and only differences in the
    // definitions themselves are found
    let mut new = new.clone();
    new.ext_crates[0] = old_version.clone();
    // the format a snapshot was saved in is not part of the definitions
    new.ww_self_version = old.ww_self_version;
    let mut checker = Checker {
        old,
        new: &new,
        breaking: vec![],
        compatible: vec![],
        warnings: vec![],
        new_version: triplet(&new_version.version),
        old_version: triplet(&old_version.version),
    };
    checker.compare_traits();
    checker.compare_types();

    let change = if !checker.breaking.is_empty() {
        Change::Breaking
    } else if bytes(old, false)? != bytes(&new, false)? {
        if checker.compatible.is_empty() {
            checker
                .compatible
                .push("definitions changed, e.g. #[since], #[default] values, argument names or order of items".into());
        }
        Change::Compatible
    } else if bytes(old, true)? != bytes(&new, true)? {
        Change::DocsOnly
    } else {
        Change::None
    };
    Ok(Report {
        old: old_version.clone(),
        new: new_version.clone(),
        change,
        breaking: checker.breaking,
        compatible: checker.compatible,
        warnings: checker.warnings,
    })
}

struct Checker<'a> {
    old: &'a ApiBundleOwned,
    new: &'a ApiBundleOwned,
    breaking: Vec<String>,
    compatible: Vec<String>,
    warnings: Vec<String>,
    old_version: (u32, u32, u32),
    new_version: (u32, u32, u32),
}

impl<'a> Checker<'a> {
    fn compare_traits(&mut self) {
        let old_traits = own_traits(self.old);
        let new_traits = own_traits(self.new);
        for (name, old_level) in &old_traits {
            match new_traits.get(name) {
                Some(new_level) => self.compare_levels(name, old_level, new_level),
                None => self.breaking.push(format!("trait {name} removed")),
            }
        }
        for name in new_traits.keys() {
            if !old_traits.contains_key(name) {
                self.compatible.push(format!("trait {name} added"));
            }
        }
    }

    fn compare_levels(&mut self, name: &str, old: &'a ApiLevelOwned, new: &'a ApiLevelOwned) {
        let new_items: HashMap<u32, &ApiItemOwned> =
            new.items.iter().map(|item| (item.id.0, item)).collect();
        for old_item in &old.items {
            let path = format!("{name}::{}", old_item.ident);
            let Some(new_item) = new_items.get(&old_item.id.0) else {
                self.breaking.push(format!("{path} removed"));
                continue;
            };
            if old_item.ident != new_item.ident {
                if let Some(moved) = new.items.iter().find(|i| i.ident == old_item.ident) {
                    self.breaking.push(format!(
                        "{path} moved from id {} to {} (resources are identified by position, add new ones at the end)",
                        old_item.id.0, moved.id.0
                    ));
                    continue;
                }
                self.breaking.push(format!(
                    "{path} renamed to {} (wire-compatible, but breaks Rust code)",
                    new_item.ident
                ));
            }
            // both ways: old client with a new server, and new client with an old server; a lowered max length
            // breaks what the old version writes, a raised one is compatible
            let mut bounds = Bounds {
                strict_writer: Some(self.old),
                narrower: vec![],
            };
            let result = compare_items(
                (self.old, old_item, OLD),
                (self.new, new_item, NEW),
                &mut bounds,
            )
            .and_then(|_| {
                compare_items(
                    (self.new, new_item, NEW),
                    (self.old, old_item, OLD),
                    &mut bounds,
                )
            });
            if let Err(e) = result {
                self.breaking.push(format!("{path}: {e}"));
            }
            self.bounds_raised(&path, bounds.narrower);
            self.compare_error_lists(&path, old_item, new_item);
        }
        for new_item in &new.items {
            // moved ones are already reported
            let is_new = |old_item: &ApiItemOwned| {
                old_item.id != new_item.id && old_item.ident != new_item.ident
            };
            if old.items.iter().all(is_new) {
                let path = format!("{name}::{}", new_item.ident);
                self.compatible.push(format!("{path} added"));
                self.check_since(&path, new_item.since);
            }
        }
    }

    /// An `#[errors(..)]` list (API-32) of a method, stream function or property changing is compatible: nothing on the wire changes and the device
    /// never checks it. Reported, since a client of the old version warns about the variants the new one adds.
    fn compare_error_lists(&mut self, path: &str, old: &ApiItemOwned, new: &ApiItemOwned) {
        let (old_list, new_list) = (old.kind.error_list(), new.kind.error_list());
        if old_list == new_list {
            return;
        }
        let entries = |list: Option<&[ErrorVariantOwned]>| -> Vec<String> {
            list.unwrap_or_default()
                .iter()
                .flat_map(|v| v.entries())
                .collect()
        };
        let change = match (old_list, new_list) {
            (None, Some(list)) => format!("#[errors({})] added", fmt_error_list(list)),
            (Some(_), None) => "#[errors(..)] removed, any variant can be returned".to_string(),
            _ => {
                let (old_entries, new_entries) = (entries(old_list), entries(new_list));
                let added: Vec<_> = new_entries
                    .iter()
                    .filter(|e| !old_entries.contains(e))
                    .map(String::as_str)
                    .collect();
                let removed: Vec<_> = old_entries
                    .iter()
                    .filter(|e| !new_entries.contains(e))
                    .map(String::as_str)
                    .collect();
                let mut parts = vec![];
                if !added.is_empty() {
                    parts.push(format!(
                        "adds {} (clients of the old version warn about them)",
                        added.join(", ")
                    ));
                }
                if !removed.is_empty() {
                    parts.push(format!("drops {}", removed.join(", ")));
                }
                if parts.is_empty() {
                    parts.push("reordered".into());
                }
                format!("#[errors(..)] {}", parts.join(", "))
            }
        };
        self.compatible.push(format!("{path}: {change}"));
    }

    fn compare_types(&mut self) {
        let old_types = own_types(self.old);
        let new_types = own_types(self.new);
        for (name, old_ty) in &old_types {
            let Some(new_ty) = new_types.get(name) else {
                self.breaking.push(format!("type {name} removed"));
                continue;
            };
            // written by the old version and read by the new one, and the other way around
            let mut new_to_old = TypeCheck::new(self.new, self.old, NEW, OLD);
            let result = TypeCheck::new(self.old, self.new, OLD, NEW)
                .strict_bounds(true)
                .check(old_ty, new_ty)
                .and_then(|_| new_to_old.check(new_ty, old_ty));
            if let Err(e) = result {
                self.breaking.push(e);
            }
            self.bounds_raised(&format!("type {name}"), new_to_old.narrower);
            self.compare_idents(name, old_ty, new_ty);
        }
        for name in new_types.keys() {
            if !old_types.contains_key(name) {
                self.compatible.push(format!("type {name} added"));
            }
        }
    }

    /// Max lengths the new version raised (or removed): compatible, everything the old version writes is still
    /// read, but values the new version writes beyond the old bound are rejected by the old version.
    fn bounds_raised(&mut self, path: &str, narrower: Vec<String>) {
        for msg in narrower {
            self.compatible
                .push(format!("{path}: max length raised ({msg})"));
        }
    }

    /// Wire compatibility doesn't depend on field and variant names, but Rust code does.
    fn compare_idents(&mut self, name: &str, old: &TypeOwned, new: &TypeOwned) {
        match (old, new) {
            (TypeOwned::Struct(old), TypeOwned::Struct(new)) => {
                let unsized_ = matches!(new.size, ElementSize::Unsized);
                self.compare_fields(name, unsized_, &old.fields, &new.fields);
            }
            (TypeOwned::Enum(old), TypeOwned::Enum(new)) => {
                let unsized_ = matches!(new.size, ElementSize::Unsized);
                for new_variant in &new.variants {
                    let path = format!("{name}::{}", new_variant.ident);
                    match old
                        .variants
                        .iter()
                        .find(|v| v.discriminant == new_variant.discriminant)
                    {
                        Some(old_variant) => {
                            if old_variant.ident != new_variant.ident {
                                self.breaking.push(format!(
                                    "{name}::{} renamed to {} (wire-compatible, but breaks Rust code)",
                                    old_variant.ident, new_variant.ident
                                ));
                            }
                            self.compare_fields(
                                &path,
                                unsized_,
                                &old_variant.fields,
                                &new_variant.fields,
                            );
                        }
                        None => self.check_since(&path, new_variant.since),
                    }
                }
            }
            _ => {}
        }
    }

    /// Fields can be added in between old ones, into unused padding bits, and at the end for `Unsized` types
    /// (checked for the wire by [TypeCheck]).
    fn compare_fields(&mut self, name: &str, unsized_: bool, old: &FieldsOwned, new: &FieldsOwned) {
        let kind = |fields: &FieldsOwned| match fields {
            FieldsOwned::Named(_) => "named fields",
            FieldsOwned::Unnamed(_) => "unnamed fields",
            FieldsOwned::Unit => "no fields",
        };
        // Unit -> Named is fine as long as all the new fields have defaults, which is checked for the wire
        let (old_kind, new_kind) = (kind(old), kind(new));
        if old_kind != new_kind && !matches!(old, FieldsOwned::Unit) {
            self.breaking.push(format!(
                "{name}: {old_kind} changed to {new_kind} (breaks Rust code)"
            ));
            return;
        }
        let (old, new) = (fields(old), fields(new));
        if old.len() < new.len()
            && let Some(last_old) = layout::last_old_field(old, new)
            && (!unsized_ || last_old + 1 != old.len())
        {
            for (i, new_field) in new.iter().enumerate() {
                if !old.iter().any(|f| f.ident == new_field.ident) {
                    let path = format!("{name}: field {}", field_name(i, new_field));
                    if i < last_old {
                        self.compatible
                            .push(format!("{path} added into unused padding bits"));
                    } else {
                        self.compatible.push(format!("{path} added"));
                    }
                    self.check_since(&path, new_field.since);
                }
            }
            return;
        }
        for (i, (old_field, new_field)) in old.iter().zip(new).enumerate() {
            if old_field.ident != new_field.ident {
                self.breaking.push(format!(
                    "{name}: field {} renamed to {} (wire-compatible, but breaks Rust code)",
                    field_name(i, old_field),
                    field_name(i, new_field)
                ));
            }
        }
        for (i, new_field) in new.iter().enumerate().skip(old.len()) {
            let path = format!("{name}: field {}", field_name(i, new_field));
            self.compatible.push(format!("{path} added"));
            self.check_since(&path, new_field.since);
        }
    }

    /// Additions are expected to be marked with `#[since]` of the version they first appeared in.
    fn check_since(&mut self, path: &str, since: Option<VersionTriplet>) {
        let (major, minor, patch) = self.new_version;
        let hint = format!("#[since = \"{major}.{minor}.{patch}\"]");
        match since {
            None => self
                .warnings
                .push(format!("{path} is added without {hint}")),
            Some(since) => {
                let since = (since.major.0, since.minor.0, since.patch.0);
                if since <= self.old_version || since > self.new_version {
                    self.warnings.push(format!(
                        "{path} is added with #[since = \"{}.{}.{}\"], expected {hint}",
                        since.0, since.1, since.2
                    ));
                }
            }
        }
    }
}

/// In-line traits defined in the crate itself, by name.
fn own_traits(bundle: &ApiBundleOwned) -> BTreeMap<&str, &ApiLevelOwned> {
    bundle
        .traits
        .iter()
        .filter_map(|location| match location {
            ApiLevelLocationOwned::InLine { level, crate_idx } if crate_idx.0 == 0 => {
                Some((level.trait_name.as_str(), level))
            }
            _ => None,
        })
        .collect()
}

/// In-line structs and enums defined in the crate itself, by name.
fn own_types(bundle: &ApiBundleOwned) -> BTreeMap<&str, &TypeOwned> {
    bundle
        .types
        .iter()
        .filter_map(|location| match location {
            TypeLocationOwned::InLine { ty, crate_idx } if crate_idx.0 == 0 => match ty {
                TypeOwned::Struct(s) => Some((s.ident.as_str(), ty)),
                TypeOwned::Enum(e) => Some((e.ident.as_str(), ty)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn bytes(bundle: &ApiBundleOwned, with_docs: bool) -> Result<Vec<u8>, String> {
    let mut bundle = bundle.clone();
    if !with_docs {
        DropDocs.visit_api_bundle(&mut bundle);
    }
    bundle.to_ww_bytes_owned().map_err(|e| format!("{e:?}"))
}

/// Removes all doc comments from a bundle.
pub struct DropDocs;

impl VisitMut for DropDocs {
    fn visit_docs(&mut self, docs: &mut Vec<String>) {
        docs.clear();
    }
}

fn triplet(v: &VersionOwned) -> (u32, u32, u32) {
    (v.major.0, v.minor.0, v.patch.0)
}

fn display(v: &VersionOwned) -> String {
    format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0)
}

/// Position bumped between two versions.
fn bump(old: &VersionOwned, new: &VersionOwned) -> Bump {
    // (breaking position, compatible position)
    let positions = |v: &VersionOwned| match triplet(v) {
        (0, minor, patch) => ((0, minor), patch),
        (major, minor, _) => ((major, 0), minor),
    };
    let ((old_breaking, old_compatible), (new_breaking, new_compatible)) =
        (positions(old), positions(new));
    match new_breaking.cmp(&old_breaking) {
        Ordering::Greater => Bump::Breaking,
        Ordering::Equal if new_compatible > old_compatible => Bump::Compatible,
        _ => Bump::None,
    }
}

/// Smallest version with the required position bumped.
fn suggest(old: &VersionOwned, required: Bump) -> VersionOwned {
    let (major, minor, patch) = triplet(old);
    match (required, major) {
        (Bump::None, _) => VersionOwned::new(major, minor, patch),
        (Bump::Compatible, 0) => VersionOwned::new(0, minor, patch + 1),
        (Bump::Compatible, _) => VersionOwned::new(major, minor + 1, 0),
        (Bump::Breaking, 0) => VersionOwned::new(0, minor + 1, 0),
        (Bump::Breaking, _) => VersionOwned::new(major + 1, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Load a crate bundle from source, as `ww api save` would, by writing a throwaway crate.
    fn load(version: &str, lib_rs: &str) -> ApiBundleOwned {
        let dir: PathBuf = std::env::temp_dir()
            .join("ww_client_evolution_tests")
            .join(format!(
                "{version}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\n\n[dependencies]\n"
            ),
        )
        .unwrap();
        let lib_rs = format!("use wire_weaver::prelude::*;\n\n{lib_rs}");
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        let bundle = wire_weaver_core::load_crate(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        bundle
    }

    fn report(old: &str, new: &str) -> Report {
        compare(&load("0.1.0", old), &load("0.1.0", new)).unwrap()
    }

    fn assert_breaking(report: &Report, reason_contains: &str) {
        assert_eq!(report.change, Change::Breaking, "{report:?}");
        assert!(
            report.breaking.iter().any(|r| r.contains(reason_contains)),
            "{:?} does not contain '{reason_contains}'",
            report.breaking
        );
    }

    const API: &str = r#"
        #[ww_api_root]
        pub trait Api {
            /// Turn on
            fn led_on();
            fn set(c: Coord);
        }

        #[derive_shrink_wrap]
        pub struct Coord {
            pub x: u8,
            pub y: u8,
        }

        #[derive_shrink_wrap(ww_repr = u4)]
        pub enum Mode {
            A,
            B(u8),
        }
    "#;

    #[test]
    fn no_changes() {
        let report = report(API, API);
        assert_eq!(report.change, Change::None);
        assert!(report.check_version().is_ok());
    }

    const GENERIC: &str = r#"
        #[derive_shrink_wrap]
        pub struct Timed<T> {
            pub t: u32,
            pub value: T,
        }

        #[derive_shrink_wrap]
        pub struct Window<const N: usize> {
            pub samples: [u16; N],
        }
    "#;

    /// API-43: parameters are compared by position and kind, their names don't matter.
    #[test]
    fn generic_definitions() {
        assert_eq!(report(GENERIC, GENERIC).change, Change::None);
        let renamed = GENERIC
            .replace("Timed<T>", "Timed<V>")
            .replace("value: T", "value: V");
        assert_ne!(report(GENERIC, &renamed).change, Change::Breaking);
        let more = GENERIC
            .replace("Timed<T>", "Timed<T, U>")
            .replace("pub t: u32", "pub t: U");
        assert_breaking(&report(GENERIC, &more), "parameters <T> vs <T, U>");
        let other_kind = GENERIC.replace("const N: usize", "const N: u8");
        assert_breaking(&report(GENERIC, &other_kind), "parameters");
        let fixed = GENERIC
            .replace("Window<const N: usize>", "Window")
            .replace("[u16; N]", "[u16; 4]");
        assert_breaking(&report(GENERIC, &fixed), "parameters");
    }

    #[test]
    fn docs_only_need_a_compatible_bump() {
        let new = API.replace("/// Turn on", "/// Turn the LED on");
        let report = report(API, &new);
        assert_eq!(report.change, Change::DocsOnly);
        let err = report.check_version().unwrap_err();
        assert!(err.contains("0.1.1 or later"), "{err}");

        let report = compare(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
        assert_eq!(report.change, Change::DocsOnly);
        assert!(report.check_version().is_ok());
    }

    #[test]
    fn new_resource_is_compatible() {
        let new = API.replace(
            "fn set(c: Coord);",
            "fn set(c: Coord);\n#[since = \"0.1.1\"]\nfn led_off();",
        );
        let report = compare(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(report.compatible, ["Api::led_off added"]);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.check_version().is_ok());

        let new = new.replace("#[since = \"0.1.1\"]", "");
        let report = compare(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
        assert_eq!(report.change, Change::Compatible);
        assert_eq!(
            report.warnings,
            ["Api::led_off is added without #[since = \"0.1.1\"]"]
        );
    }

    #[test]
    fn max_length_raised_is_compatible_lowered_is_breaking() {
        let api = |data: &str, name: &str, field: &str| {
            format!(
                r#"
                #[ww_api_root]
                pub trait Api {{
                    fn write(data: {data}) -> {name};
                    property!(rw label: MaxLen<&'i str, 4>);
                }}

                #[derive_shrink_wrap]
                pub struct Piece<'i> {{
                    pub data: {field},
                }}
            "#
            )
        };
        let bytes = |n: u32| format!("MaxLen<RefVec<'i, u8>, {n}>");
        let name = |n: u32| format!("MaxLen<&'i str, {n}>");
        let old = api(&bytes(8), &name(8), &bytes(8));
        assert_eq!(report(&old, &old).change, Change::None);

        // argument, return value and struct field raised: what the old version writes is still read
        let raised = api(&bytes(16), &name(16), &bytes(16));
        let report_raised = report(&old, &raised);
        assert_eq!(
            report_raised.change,
            Change::Compatible,
            "{report_raised:?}"
        );
        let compatible = report_raised.compatible.join("\n");
        assert!(
            compatible.contains("Api::write: max length raised"),
            "{compatible}"
        );
        assert!(
            compatible.contains("type Piece: max length raised"),
            "{compatible}"
        );
        // removing a bound is raising it to none
        let removed = api("RefVec<'i, u8>", "&'i str", "RefVec<'i, u8>");
        assert_eq!(report(&old, &removed).change, Change::Compatible);

        // lowered: the old version writes values the new one rejects
        for lowered in [
            api(&bytes(4), &name(8), &bytes(8)),
            api(&bytes(8), &name(4), &bytes(8)),
            api(&bytes(8), &name(8), &bytes(4)),
        ] {
            assert_breaking(
                &report(&old, &lowered),
                "longer values the old version writes",
            );
        }
        // adding a bound is lowering it from none
        assert_breaking(&report(&removed, &old), "no max length on the old version");
    }

    #[test]
    fn changed_series_element_is_breaking() {
        let api = |ty: &str| {
            format!(
                r#"
                #[ww_api_root]
                pub trait Api {{
                    fn samples() -> {ty};
                }}
            "#
            )
        };
        // the codecs are picked per column when writing, they are not part of the type
        let old = api("Series<'i, u32>");
        assert_eq!(report(&old, &old).change, Change::None);
        assert_eq!(report(&old, &api("SeriesOwned<u32>")).change, Change::None);
        for (new, reason) in [
            ("Series<'i, u64>", "u64"),
            ("Series<'i, i32>", "i32"),
            ("RefVec<'i, u32>", "Vec"),
        ] {
            assert_breaking(&report(&old, &api(new)), reason);
        }
        let old = api("Series<'i, f32>");
        assert_breaking(&report(&old, &api("Series<'i, f64>")), "f64");
    }

    #[test]
    fn removed_or_renamed_resource_is_breaking() {
        let new = API.replace("fn set(c: Coord);", "");
        assert_breaking(&report(API, &new), "Api::set removed");

        let new = API.replace("fn led_on();", "fn led_off();");
        let report = compare(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
        assert_breaking(&report, "Api::led_on renamed to led_off");
        let err = report.check_version().unwrap_err();
        assert!(err.contains("0.2.0 or later"), "{err}");
        let report = compare(&load("0.1.0", API), &load("0.2.0", &new)).unwrap();
        assert!(report.check_version().is_ok());
    }

    #[test]
    fn inserted_resource_moves_the_rest() {
        let new = API.replace(
            "fn led_on();",
            "fn led_on();\n#[since = \"0.1.1\"]\nfn led_off();",
        );
        let report = self::report(API, &new);
        assert_eq!(
            report.breaking,
            [
                "Api::set moved from id 1 to 2 (resources are identified by position, add new ones at the end)"
            ]
        );
    }

    #[test]
    fn changed_argument_is_breaking() {
        let new = API.replace("fn set(c: Coord);", "fn set(c: u8);");
        assert_breaking(&report(API, &new), "Api::set: argument `c`");
    }

    #[test]
    fn struct_fields() {
        let with_default = API.replace(
            "pub y: u8,",
            "pub y: u8,\n#[default = None]\n#[since = \"0.1.1\"]\npub z: Option<u8>,",
        );
        let report = compare(&load("0.1.0", API), &load("0.1.1", &with_default)).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let no_default = API.replace("pub y: u8,", "pub y: u8,\npub z: Option<u8>,");
        assert_breaking(
            &self::report(API, &no_default),
            "struct Coord: field `z` is not sent by the old version and has no #[default]",
        );

        let renamed = API.replace("pub y: u8,", "pub yy: u8,");
        assert_breaking(
            &self::report(API, &renamed),
            "Coord: field `y` renamed to `yy`",
        );
    }

    #[test]
    fn sized_struct_fields_in_padding() {
        let v1 = r#"
            #[derive_shrink_wrap(sized)]
            pub struct S {
                pub a: u8,
                pub f: bool,
                pub b: u8,
            }
        "#;
        // 7 bits of padding after `f`, wherever S starts, as `a` is byte-aligned
        let v2 = v1.replace(
            "pub f: bool,",
            "pub f: bool,\n#[since = \"0.1.1\"]\npub g: u3,",
        );
        let report = compare(&load("0.1.0", v1), &load("0.1.1", &v2)).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(
            report.compatible,
            ["S: field `g` added into unused padding bits"]
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // fills all 7 bits
        let v2 = v1.replace("pub f: bool,", "pub f: bool,\npub g: u7,");
        assert_eq!(self::report(v1, &v2).change, Change::Compatible);
        // one bit too many
        let v2 = v1.replace("pub f: bool,", "pub f: bool,\npub g: u8,");
        assert_breaking(
            &self::report(v1, &v2),
            "doesn't fit into unused padding bits",
        );
        // at the end, shifts whatever follows S in a parent
        let v2 = v1.replace("pub b: u8,", "pub b: u8,\npub g: bool,");
        assert_breaking(
            &self::report(v1, &v2),
            "doesn't fit into unused padding bits",
        );

        // only 2 bits of padding when S starts at bit offset 5
        let v1 = "#[derive_shrink_wrap(sized)]\npub struct S { pub a: bool, pub b: u8 }";
        let v2 = "#[derive_shrink_wrap(sized)]\npub struct S { pub a: bool, pub c: u3, pub b: u8 }";
        assert_breaking(&self::report(v1, v2), "starting at bit offset 5");
    }

    #[test]
    fn sized_to_self_describing() {
        // same bytes: neither gets a size slot from its parent
        let v1 = r#"
            #[derive_shrink_wrap(sized, ww_repr = u2)]
            pub enum E { A, B(u8) }
            #[derive_shrink_wrap(sized)]
            pub struct S { pub a: u8 }
        "#;
        let v2 = v1.replace("sized", "self_describing");
        let report = self::report(v1, &v2);
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        let report = self::report(&v2, v1);
        assert_eq!(report.change, Change::Compatible, "{report:?}");

        // a size slot appears or goes away
        let v2 = v1.replace("sized, ww_repr = u2", "ww_repr = u2");
        assert_breaking(&self::report(v1, &v2), "Sized { size_bits: 0 } vs Unsized");
        // FinalStructure is a promise about the fields, kept apart
        let v2 = v1.replace("(sized)", "(final_structure)");
        assert_breaking(&self::report(v1, &v2), "vs UnsizedFinalStructure");
    }

    #[test]
    fn unsized_struct_fields_in_padding() {
        let v1 = r#"
            #[derive_shrink_wrap]
            pub struct U {
                pub f: bool,
                pub b: u8,
                pub s: String,
            }
        "#;
        // Unsized types always start at a byte boundary, 7 bits of padding after `f`
        let v2 = v1.replace(
            "pub f: bool,",
            "pub f: bool,\n#[since = \"0.1.1\"]\npub g: u7,",
        );
        let report = compare(&load("0.1.0", v1), &load("0.1.1", &v2)).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(
            report.compatible,
            ["U: field `g` added into unused padding bits"]
        );

        let too_big = v1.replace("pub f: bool,", "pub f: bool,\npub g: u8,");
        assert_breaking(
            &self::report(v1, &too_big),
            "new field `g` doesn't fit into unused padding bits",
        );

        // in between and at the end at the same time
        let both = v2.replace(
            "pub s: String,",
            "pub s: String,\n#[default = None]\n#[since = \"0.1.1\"]\npub z: Option<u8>,",
        );
        let report = compare(&load("0.1.0", v1), &load("0.1.1", &both)).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(
            report.compatible,
            [
                "U: field `g` added into unused padding bits",
                "U: field `z` added"
            ]
        );
        let no_default = both.replace("#[default = None]", "");
        assert_breaking(
            &self::report(v1, &no_default),
            "field `z` is not sent by the old version and has no #[default]",
        );
    }

    #[test]
    fn enum_variants() {
        let new = API.replace("B(u8),", "B(u8),\nC,");
        assert_breaking(
            &report(API, &new),
            "variant C is unknown to the old version",
        );
        let new = API.replace("B(u8),", "Bb(u8),");
        assert_breaking(&report(API, &new), "Mode::B renamed to Bb");
    }

    #[test]
    fn types_added_and_removed() {
        let new = format!("{API}\n#[derive_shrink_wrap]\npub struct Extra {{ pub a: u8 }}");
        let report = self::report(API, &new);
        assert_eq!(report.change, Change::Compatible);
        assert_eq!(report.compatible, ["type Extra added"]);
        assert_breaking(&self::report(&new, API), "type Extra removed");
    }

    #[test]
    fn version_positions() {
        let v = VersionOwned::new;
        assert_eq!(bump(&v(0, 1, 0), &v(0, 1, 1)), Bump::Compatible);
        assert_eq!(bump(&v(0, 1, 0), &v(0, 2, 0)), Bump::Breaking);
        assert_eq!(bump(&v(0, 1, 3), &v(1, 0, 0)), Bump::Breaking);
        assert_eq!(
            bump(&v(1, 1, 0), &v(1, 1, 1)),
            Bump::None,
            "patch after 1.0"
        );
        assert_eq!(bump(&v(1, 1, 0), &v(1, 2, 0)), Bump::Compatible);
        assert_eq!(bump(&v(1, 1, 0), &v(2, 0, 0)), Bump::Breaking);
        assert_eq!(triplet(&suggest(&v(1, 1, 3), Bump::Compatible)), (1, 2, 0));
        assert_eq!(triplet(&suggest(&v(0, 1, 3), Bump::Breaking)), (0, 2, 0));
    }

    #[test]
    fn diff_lists_every_change() {
        assert_eq!(diff(&load("0.1.0", API), &load("0.1.0", API)).unwrap(), []);

        let new = API
            .replace("/// Turn on", "/// Turn the LED on\n/// and keep it on")
            .replace(
                "fn set(c: Coord);",
                "fn set(c: Coord) -> u8;\n#[since = \"0.1.1\"]\nfn led_off();",
            )
            .replace("pub x: u8,", "/// Horizontal\npub x: u16,")
            .replace(
                "pub y: u8,",
                "pub y: u8,\n#[default = None]\npub z: Option<u8>,",
            )
            .replace("B(u8),", "Bb(u8),")
            .replace("ww_repr = u4", "ww_repr = u8");
        let diff = diff(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
        let changed = |path: &str, what, old: &str, new: &str| Difference::Changed {
            path: path.into(),
            what,
            old: old.into(),
            new: new.into(),
        };
        let lines = |lines: &[&str]| lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        assert_eq!(
            diff,
            [
                Difference::Docs {
                    path: "Api::led_on".into(),
                    old: lines(&["Turn on"]),
                    new: lines(&["Turn the LED on", "and keep it on"]),
                },
                changed(
                    "Api::set",
                    "signature",
                    "fn set(c: Coord)",
                    "fn set(c: Coord) -> u8"
                ),
                Difference::Added {
                    path: "Api::led_off".into(),
                    definition: "fn led_off()".into()
                },
                changed("Coord: field `x`", "type", "u8", "u16"),
                Difference::Docs {
                    path: "Coord: field `x`".into(),
                    old: vec![],
                    new: lines(&["Horizontal"]),
                },
                Difference::Added {
                    path: "Coord: field `z`".into(),
                    definition: "z: Option<u8>".into()
                },
                changed(
                    "type Mode",
                    "kind",
                    "enum Mode (unsized, repr u4)",
                    "enum Mode (unsized, repr u8)"
                ),
                changed("Mode::B", "name", "B", "Bb"),
            ]
        );
    }

    /// `Api` with `items` and the error enums of [error_lists].
    fn error_list_api(items: &str) -> String {
        format!(
            r#"
                #[ww_api_root]
                pub trait Api {{
                    {items}
                }}

                #[derive_shrink_wrap(ww_repr = u4)]
                pub enum ReadError {{
                    Common(Inner),
                    Nack,
                    Overrun,
                }}

                #[derive_shrink_wrap(ww_repr = u4)]
                pub enum Inner {{
                    Busy,
                    InUse,
                }}
                "#
        )
    }

    /// An `#[errors(..)]` list of a stream function or a property changing is compatible and reported, as for
    /// methods (API-32).
    #[test]
    fn error_lists_of_streams_and_properties() {
        let compatible = |old: &str, new: &str| {
            let report = report(&error_list_api(old), &error_list_api(new));
            assert_eq!(report.change, Change::Compatible, "{report:?}");
            report.compatible
        };
        let stream = "#[stream(u8)] fn samples() -> Result<(), ReadError>;";
        assert_eq!(
            compatible(stream, &format!("#[errors(Nack)] {stream}")),
            ["Api::samples: #[errors(Nack)] added"]
        );
        let property = "property!(rw mode: u8, ReadError);";
        assert_eq!(
            compatible(
                &format!("#[errors(Nack)] {property}"),
                &format!("#[errors(Nack, Overrun)] {property}")
            ),
            ["Api::mode: #[errors(..)] adds Overrun (clients of the old version warn about them)"]
        );
    }

    /// A method's `#[errors(..)]` list changing is compatible and reported (API-32).
    #[test]
    fn error_lists() {
        let api = |errors: &str| {
            format!(
                r#"
                #[ww_api_root]
                pub trait Api {{
                    {errors}
                    fn read() -> Result<u8, ReadError>;
                }}

                #[derive_shrink_wrap(ww_repr = u4)]
                pub enum ReadError {{
                    Common(Inner),
                    Nack,
                    Overrun,
                }}

                #[derive_shrink_wrap(ww_repr = u4)]
                pub enum Inner {{
                    Busy,
                    InUse,
                }}
                "#
            )
        };
        let compatible = |old: &str, new: &str| {
            let report = report(&api(old), &api(new));
            assert_eq!(report.change, Change::Compatible, "{report:?}");
            report.compatible
        };
        assert_eq!(
            compatible("", "#[errors(Nack)]"),
            ["Api::read: #[errors(Nack)] added"]
        );
        assert_eq!(
            compatible("#[errors(Nack)]", ""),
            ["Api::read: #[errors(..)] removed, any variant can be returned"]
        );
        assert_eq!(
            compatible(
                "#[errors(Common(Busy), Nack)]",
                "#[errors(Common(Busy, InUse))]"
            ),
            [
                "Api::read: #[errors(..)] adds Common(InUse) (clients of the old version warn about them), drops Nack"
            ]
        );
        assert_eq!(
            compatible("#[errors(Nack, Overrun)]", "#[errors(Overrun, Nack)]"),
            ["Api::read: #[errors(..)] reordered"]
        );

        let diff = diff(
            &load("0.1.0", &api("#[errors(Nack)]")),
            &load("0.1.1", &api("#[errors(Common, Nack)]")),
        )
        .unwrap();
        assert_eq!(
            diff,
            [Difference::Changed {
                path: "Api::read".into(),
                what: "#[errors]",
                old: "#[errors(Nack)]".into(),
                new: "#[errors(Common, Nack)]".into(),
            }]
        );
    }

    /// Consecutive versions of each crate in the embedded snapshots follow the rules.
    #[test]
    fn embedded_snapshots_evolve_correctly() {
        let mut by_crate: HashMap<&str, Vec<&ApiBundleOwned>> = HashMap::new();
        for (version, bundle) in crate::snapshots::all() {
            by_crate.entry(&version.crate_id).or_default().push(bundle);
        }
        for bundles in by_crate.values_mut() {
            bundles.sort_by_key(|b| triplet(&b.ext_crates[0].version));
            for pair in bundles.windows(2) {
                let report = compare(pair[0], pair[1]).unwrap();
                report
                    .check_version()
                    .unwrap_or_else(|e| panic!("{e}: {:?}", report.breaking));
            }
        }
    }
}
