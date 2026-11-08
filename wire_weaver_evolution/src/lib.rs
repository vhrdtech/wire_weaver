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
//! The wire version (`#[ww_api_root(wire = "w2.3")]`, or `[package.metadata.wire_weaver] wire` in `Cargo.toml`) is
//! checked apart from the crate version, see [WireBump] (EVO-27):
//!
//! * A new line (`w2.3` -> `w3.0`): a change that breaks the wire, as above, but not one that only breaks Rust code
//!   (a rename, an argument added at the end with a default).
//! * A new revision (`w2.3` -> `w2.4`): a wire-compatible addition, e.g. a new method, a field or a trailing argument
//!   with a default, a dependency crate at a newer revision of its line.
//! * Nothing else needs a wire bump. A line bumped by hand for a change of meaning is always accepted.
//!
//! [diff] lists every difference instead, doc comments included, without classifying them.
//!
//! With the default `source` feature, [Check] parses an API crate from source and compares it with the snapshots in
//! its `api_snapshots/`, as `ww api check` does. An API crate runs it from a test of its own, so its lock file pins
//! the checker and `cargo test`, CI and a pre-commit hook all check the same way:
//!
//! ```no_run
//! // tests/evolution.rs of an API crate: `WW_API_SAVE=1 cargo test --test evolution` saves the current version's
//! // snapshot, plain `cargo test` fails without it
//! #[test]
//! fn api_evolution() {
//!     wire_weaver_evolution::Check::new(env!("CARGO_MANIFEST_DIR")).require_current(true).assert();
//! }
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use shrink_wrap::{ElementSize, SerializeShrinkWrapOwned};
use ww_self::errors::fmt_error_list;
use ww_self::visit_mut::VisitMut;
use ww_self::{
    ApiBundleOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned, ClaimMode,
    ErrorVariantOwned, FieldsOwned, FutureVersionOwned, MetaOwned, TypeLocationOwned, TypeOwned,
};
use ww_version::{FullVersionOwned, VersionOwned, WireVersion};

use crate::compat::{
    Bounds, TypeCheck, Unwrapped, compare_items, field_name, fields, signature_change,
};
use crate::recorded::{Recorded, Unrecorded};
pub use compat::SignatureChange;

#[cfg(feature = "source")]
mod check;
pub mod compat;
mod diff;
mod layout;
pub mod recorded;
mod rule;
pub mod snapshot_files;

#[cfg(feature = "source")]
pub use check::{
    Check, Comparison, NoSnapshot, Outcome, SAVE_ENV, SaveStatus, Saved, UnitHint,
    missing_test_warning,
};
pub use compat::RootCause;
pub use diff::{Difference, diff};
pub use rule::{Finding, Rule};

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

/// Wire version position that has to be bumped (EVO-27), from the least to the most severe.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WireBump {
    /// Nothing on the wire changed: doc comments, renames, a `#[flatten]` promotion, error lists.
    None,
    /// A new revision on the line (`w2.3` -> `w2.4`): wire-compatible additions.
    Revision,
    /// A new line (`w2.3` -> `w3.0`): a wire break, peers of the two versions no longer talk.
    Line,
}

impl WireBump {
    /// The smallest wire version after `old` with this bump.
    pub fn apply(&self, old: WireVersion) -> WireVersion {
        match self {
            WireBump::None => old,
            WireBump::Revision => old.next_revision(),
            WireBump::Line => old.next_line(),
        }
    }

    /// Bump between two wire versions, [WireBump::None] when `new` is not newer.
    pub fn between(old: WireVersion, new: WireVersion) -> WireBump {
        if new.line.0 > old.line.0 {
            WireBump::Line
        } else if new > old {
            WireBump::Revision
        } else {
            WireBump::None
        }
    }
}

impl fmt::Display for WireBump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WireBump::None => "no wire bump",
            WireBump::Revision => "a new wire revision",
            WireBump::Line => "a new wire line",
        })
    }
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
    /// Why the change is breaking, with the rule each change breaks; empty if it isn't.
    pub breaking: Vec<Finding>,
    /// Additions and other compatible changes found.
    pub compatible: Vec<String>,
    /// Not affecting the verdict, e.g. an added resource without `#[since]`.
    pub warnings: Vec<String>,
    /// Fields of the model that one of the snapshot files compared doesn't have (saved by an older wire_weaver), left
    /// out on both sides, see [compare_recorded].
    pub unrecorded: Vec<Unrecorded>,
    /// Wire versions of the crate on each side (EVO-24, EVO-26), `None` where it declares none.
    pub old_wire: Option<WireVersion>,
    pub new_wire: Option<WireVersion>,
    /// Wire position the changes need bumped (EVO-27), whether or not the crate declares a wire version.
    pub wire_bump: WireBump,
    /// Why [Self::wire_bump] is needed: the wire breaks (several caused by one dependency named once, EVO-21) for
    /// a new line, the wire-compatible additions for a new revision.
    pub wire_reasons: Vec<String>,
    /// Items, fields and variants with a `#[since]` newer than the new wire version: a host built with it refuses
    /// them on every device of this version, which [Self::check_wire] fails on.
    pub since_too_new: Vec<String>,
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

    /// Smallest wire version the new one has to be for the changes found, `None` when the old side declares none.
    pub fn minimal_wire(&self) -> Option<WireVersion> {
        self.old_wire.map(|old| self.wire_bump.apply(old))
    }

    /// Checks that the new wire version is bumped enough for the changes found (EVO-27) and that no `#[since]` is
    /// newer than it. A crate declaring no wire version on the old side has nothing to compare with, and passes; one
    /// that stops declaring it fails. Any bump beyond what is needed is accepted, e.g. a new line by hand for a
    /// change of meaning.
    pub fn check_wire(&self) -> Result<(), String> {
        let crate_id = &self.new.crate_id;
        let mut errors = vec![];
        match (self.old_wire, self.new_wire) {
            (None, _) => {}
            (Some(old), None) => errors.push(format!(
                "{crate_id} declared wire version {old}, the new version declares none"
            )),
            (Some(old), Some(new)) if new < old => {
                errors.push(format!("{crate_id} wire version {new} is older than {old}"))
            }
            (Some(old), Some(new)) if WireBump::between(old, new) < self.wire_bump => {
                errors.push(format!(
                    "{crate_id} wire {old} -> {new} has {}, which needs {}: {} or later",
                    self.wire_reasons_short(),
                    self.wire_bump,
                    self.wire_bump.apply(old)
                ))
            }
            _ => {}
        }
        for item in &self.since_too_new {
            errors.push(item.clone());
        }
        match errors.is_empty() {
            true => Ok(()),
            false => Err(errors.join(".\n")),
        }
    }

    /// [Self::wire_reasons], the first three and how many more.
    fn wire_reasons_short(&self) -> String {
        let mut short = self.wire_reasons[..self.wire_reasons.len().min(3)].join("; ");
        if self.wire_reasons.len() > 3 {
            short.push_str(&format!(" and {} more", self.wire_reasons.len() - 3));
        }
        short
    }

    /// The bumps needed, both in one line: `wire w1.0 -> w1.1 or later (a new wire revision), crate 0.1.0 -> 0.1.1 or
    /// later (compatible position ...)`.
    pub fn required(&self) -> String {
        let wire = match (self.old_wire, self.minimal_wire()) {
            (Some(old), Some(_)) if self.wire_bump == WireBump::None => {
                format!("wire {old} (no wire bump)")
            }
            (Some(old), Some(min)) => format!("wire {old} -> {min} or later ({})", self.wire_bump),
            _ if self.wire_bump == WireBump::None => "no wire version declared".to_string(),
            _ => format!("no wire version declared ({} if it were)", self.wire_bump),
        };
        let required = self.change.required_bump();
        let crate_ = match required {
            Bump::None => format!("crate {} (no bump)", display(&self.old.version)),
            _ => format!(
                "crate {} -> {} or later ({required})",
                display(&self.old.version),
                display(&self.minimal_version())
            ),
        };
        format!("{wire}, {crate_}")
    }

    /// [Self::breaking] grouped by [Finding::root], in the order each root was first found: several items broken by
    /// the same dependency change are listed together instead of looking like unrelated breaks (EVO-21). Only roots
    /// reached by more than one finding are grouped; the rest (including every finding without one) come back as
    /// `rest`, in their original order.
    pub fn breaking_groups(&self) -> (Vec<(&RootCause, Vec<&Finding>)>, Vec<&Finding>) {
        let mut groups: Vec<(&RootCause, Vec<&Finding>)> = vec![];
        for finding in &self.breaking {
            let Some(root) = &finding.root else { continue };
            match groups.iter_mut().find(|(r, _)| *r == root) {
                Some((_, items)) => items.push(finding),
                None => groups.push((root, vec![finding])),
            }
        }
        groups.retain(|(_, items)| items.len() > 1);
        let grouped: Vec<*const Finding> = groups
            .iter()
            .flat_map(|(_, items)| items.iter().map(|f| *f as *const Finding))
            .collect();
        let rest = self
            .breaking
            .iter()
            .filter(|f| !grouped.contains(&(*f as *const Finding)))
            .collect();
        (groups, rest)
    }
}

/// [compare] of what both sides recorded: a field of the API model that either side's snapshot file doesn't have
/// (saved by a wire_weaver from before the field) is left out on both, so that upgrading wire_weaver alone never
/// shows up as a change (EVO-13). [Recorded::default] for a bundle loaded from source, which has every field.
pub fn compare_recorded(
    old: &ApiBundleOwned,
    old_recorded: &Recorded,
    new: &ApiBundleOwned,
    new_recorded: &Recorded,
) -> Result<Report, String> {
    let mut unrecorded = old_recorded.unrecorded();
    unrecorded.extend(new_recorded.unrecorded());
    if unrecorded.is_empty() {
        return compare(old, new);
    }
    let old = Recorded::leave_out(old, &unrecorded)?;
    let new = Recorded::leave_out(new, &unrecorded)?;
    // a field signatures include is missing on one side: its signatures were made over other bytes
    let signatures = unrecorded
        .iter()
        .all(|u| !ww_self::signature::includes(&u.owner, &u.field));
    let mut report = compare_with(&old, &new, signatures)?;
    report.unrecorded = unrecorded.into_iter().collect();
    Ok(report)
}

/// Compare an old version of a crate with a new one. Both are crate bundles, as loaded by
/// `wire_weaver_core::load_crate` and saved by `ww api save`.
pub fn compare(old: &ApiBundleOwned, new: &ApiBundleOwned) -> Result<Report, String> {
    compare_with(old, new, true)
}

/// [compare], checking definitions one side left out by their signatures when `signatures`.
fn compare_with(
    old: &ApiBundleOwned,
    new: &ApiBundleOwned,
    signatures: bool,
) -> Result<Report, String> {
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
        old_wire: old.wire,
        new_wire: new.wire,
        wire_additions: vec![],
        wire_lines: vec![],
        signatures,
    };
    checker.compare_wire();
    checker.compare_traits();
    checker.compare_types();
    checker.compare_dependency_wires();
    let since_too_new = checker.since_not_newer();

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
    let (wire_bump, wire_reasons) = wire_bump(
        &checker.breaking,
        checker.wire_lines,
        checker.wire_additions,
    );
    Ok(Report {
        old: old_version.clone(),
        new: new_version.clone(),
        change,
        breaking: checker.breaking,
        compatible: checker.compatible,
        warnings: checker.warnings,
        unrecorded: vec![],
        old_wire: old.wire,
        new_wire: new.wire,
        wire_bump,
        wire_reasons,
        since_too_new,
    })
}

/// [Report::wire_bump] and [Report::wire_reasons]: a new line for any finding that breaks the wire (its rule doesn't
/// [keep it](Rule::keeps_wire)) or a dependency on another line, several findings with one root named once, and
/// under the dependency's new line when the root is in that crate (EVO-21); else a new revision for any
/// wire-compatible addition.
fn wire_bump(
    breaking: &[Finding],
    lines: Vec<DependencyLine>,
    additions: Vec<String>,
) -> (WireBump, Vec<String>) {
    let mut roots: Vec<(&RootCause, usize)> = vec![];
    let mut plain = vec![];
    for finding in breaking.iter().filter(|f| !f.rule.keeps_wire()) {
        match &finding.root {
            Some(root) => match roots.iter_mut().find(|(r, _)| *r == root) {
                Some((_, n)) => *n += 1,
                None => roots.push((root, 1)),
            },
            None => plain.push(format!("a wire break ({})", finding.message)),
        }
    }
    let breaks = |n: usize| match n {
        1 => "a wire break".to_string(),
        n => format!("{n} wire breaks"),
    };
    let mut reasons = vec![];
    for line in &lines {
        let caused: Vec<String> = roots
            .iter()
            .filter(|(root, _)| root.crate_id == line.crate_id)
            .map(|(root, n)| format!("{} because {root} changed", breaks(*n)))
            .collect();
        reasons.push(match caused.is_empty() {
            true => line.message.clone(),
            false => format!("{} ({})", line.message, caused.join(", ")),
        });
    }
    for (root, n) in &roots {
        if !lines.iter().any(|line| line.crate_id == root.crate_id) {
            reasons.push(format!("{} because {root} changed", breaks(*n)));
        }
    }
    reasons.append(&mut plain);
    if !reasons.is_empty() {
        return (WireBump::Line, reasons);
    }
    match additions.is_empty() {
        true => (WireBump::None, vec![]),
        false => (WireBump::Revision, additions),
    }
}

/// A dependency crate on another wire line than before (EVO-27).
struct DependencyLine {
    crate_id: String,
    message: String,
}

struct Checker<'a> {
    old: &'a ApiBundleOwned,
    new: &'a ApiBundleOwned,
    breaking: Vec<Finding>,
    compatible: Vec<String>,
    warnings: Vec<String>,
    /// Wire versions of the compared crates (EVO-24, EVO-26), what `#[since]` names. `None` for a crate that declares
    /// none.
    old_wire: Option<WireVersion>,
    new_wire: Option<WireVersion>,
    /// Wire-compatible additions found, each a new wire revision (EVO-27), also listed in `compatible`.
    wire_additions: Vec<String>,
    /// Dependency crates on another wire line than before, each a new wire line (EVO-27).
    wire_lines: Vec<DependencyLine>,
    /// Definitions left out on one side are checked by their signatures, see [TypeCheck::compare_signatures].
    signatures: bool,
}

impl<'a> Checker<'a> {
    /// A compatible change that the wire carries: a new wire revision (EVO-27).
    fn adds(&mut self, message: String) {
        self.wire_additions.push(message.clone());
        self.compatible.push(message);
    }

    fn breaks(&mut self, rule: Rule, message: String) {
        self.breaking.push(Finding::new(rule, message));
    }

    /// [Self::breaks] naming the dependency trait, type or marker whose own definition the checker found to
    /// differ, when it could tell (EVO-21).
    fn breaks_with_root(&mut self, rule: Rule, message: String, root: Option<RootCause>) {
        self.breaking.push(Finding::with_root(rule, message, root));
    }

    fn compare_traits(&mut self) {
        let old_traits = own_traits(self.old);
        let new_traits = own_traits(self.new);
        for (name, old_level) in &old_traits {
            match new_traits.get(name) {
                Some(new_level) => self.compare_levels(name, old_level, new_level),
                None => self.breaks(Rule::Removed, format!("trait {name} removed")),
            }
        }
        for name in new_traits.keys() {
            if !old_traits.contains_key(name) {
                self.adds(format!("trait {name} added"));
            }
        }
    }

    fn compare_levels(&mut self, name: &str, old: &'a ApiLevelOwned, new: &'a ApiLevelOwned) {
        // ww_global ids (API-20): hosts and devices of both versions must mean the same trait by the same number
        match (old.compact_id, new.compact_id) {
            (Some(old_id), Some(new_id)) if old_id != new_id => self.breaks(Rule::GlobalId, format!(
                "trait {name}: ww_global id changed from {} to {} (ids never change, a renamed trait keeps its id \
                 through `aliases` in wire_weaver_gid.json)",
                old_id.0, new_id.0
            )),
            (Some(old_id), None) => self.breaks(Rule::GlobalId, format!(
                "trait {name}: ww_global id {} gone (ids are never removed, a removed trait is marked `retired`)",
                old_id.0
            )),
            (None, Some(new_id)) => {
                self.adds(format!("trait {name}: ww_global id {} assigned", new_id.0))
            }
            _ => {}
        }
        self.compare_meta(
            &format!("trait {name}"),
            old.meta.as_ref(),
            new.meta.as_ref(),
        );
        let new_items: HashMap<u32, &ApiItemOwned> =
            new.items.iter().map(|item| (item.id.0, item)).collect();
        for old_item in &old.items {
            let path = format!("{name}::{}", old_item.ident);
            let Some(new_item) = new_items.get(&old_item.id.0) else {
                self.breaks(Rule::Removed, format!("{path} removed"));
                continue;
            };
            if old_item.ident != new_item.ident {
                if let Some(moved) = new.items.iter().find(|i| i.ident == old_item.ident) {
                    self.breaks(Rule::Moved, format!(
                        "{path} moved from id {} to {} (resources are identified by position, add new ones at the end)",
                        old_item.id.0, moved.id.0
                    ));
                    continue;
                }
                self.breaks(
                    Rule::Renamed,
                    format!(
                        "{path} renamed to {} (wire-compatible, but breaks Rust code)",
                        new_item.ident
                    ),
                );
            }
            // both ways: old client with a new server, and new client with an old server; a lowered max length
            // breaks what the old version writes, a raised one is compatible
            let mut bounds = Bounds {
                strict_writer: Some(self.old),
                skip_signatures: !self.signatures,
                ..Bounds::default()
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
                let rule = match bounds.rule {
                    Some(rule) => rule,
                    None if bounds.lowered => Rule::MaxLen,
                    None => Rule::Resource,
                };
                self.breaks_with_root(rule, format!("{path}: {e}"), bounds.root.clone());
            } else if let Some(change) = signature_change(&old_item.kind, &new_item.kind) {
                // EVO-22: the wire is fine, the Rust signatures are not
                self.breaks(Rule::Signature, format!("{path}: {}", change.message));
                if change.arguments_changed {
                    self.wire_additions
                        .push(format!("{path}: arguments added or removed at the end"));
                }
            }
            self.bounds_raised(&path, bounds.narrower);
            self.unwrapped(&path, &bounds.unwrapped);
            self.compare_error_lists(&path, old_item, new_item);
            self.compare_optional(&path, old_item, new_item);
            self.compare_meta(&path, old_item.meta.as_ref(), new_item.meta.as_ref());
        }
        for new_item in &new.items {
            // moved ones are already reported
            let is_new = |old_item: &ApiItemOwned| {
                old_item.id != new_item.id && old_item.ident != new_item.ident
            };
            if old.items.iter().all(is_new) {
                let path = format!("{name}::{}", new_item.ident);
                self.adds(format!("{path} added"));
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

    /// `#[claim]`, `#[shares]` and `#[label]` (SRV-34) are not on the wire and a server doesn't use them: a change is
    /// compatible, reported for what the hub, `ww` and GUIs see.
    fn compare_meta(&mut self, path: &str, old: Option<&MetaOwned>, new: Option<&MetaOwned>) {
        let none = MetaOwned::default();
        let (old, new) = (old.unwrap_or(&none), new.unwrap_or(&none));
        let claim = |mode: Option<ClaimMode>| match mode {
            Some(mode) => format!("#[claim({})]", mode.name()),
            None => "not claimable".to_string(),
        };
        if old.claim != new.claim {
            self.compatible.push(format!(
                "{path}: {} became {}",
                claim(old.claim),
                claim(new.claim)
            ));
        }
        if old.shares != new.shares {
            self.compatible.push(format!("{path}: #[shares] changed"));
        }
        if old.labels != new.labels {
            self.compatible.push(format!("{path}: #[label] changed"));
        }
    }

    /// `#[optional]` / `#[optional(group)]` (API-16, API-54) is not on the wire, but a server leaves items out by name
    /// or group (`omit = [..]`): a change that makes such a server lack a handler is a break of its Rust code.
    fn compare_optional(&mut self, path: &str, old: &ApiItemOwned, new: &ApiItemOwned) {
        let group = |g: &Option<String>| match g {
            Some(g) => format!("#[optional({g})]"),
            None => "#[optional]".to_string(),
        };
        match (&old.optional, &new.optional) {
            (None, None) => {}
            (None, Some(o)) => self.compatible.push(format!(
                "{path}: made {} (a device may leave it out, a client gets Unimplemented)",
                group(&o.group)
            )),
            (Some(_), None) => self.breaks(Rule::Optional, format!(
                "{path}: no longer #[optional] (a server leaving it out with omit = [..] no longer compiles)"
            )),
            (Some(o), Some(n)) if o.group == n.group => {}
            (Some(o), Some(n)) if o.group.is_none() => self.compatible.push(format!(
                "{path}: #[optional] became {} (omit by its name still works)",
                group(&n.group)
            )),
            (Some(o), Some(n)) => self.breaks(Rule::Optional, format!(
                "{path}: {} became {} (a server leaving out its group with omit = [..] no longer leaves it out)",
                group(&o.group),
                group(&n.group)
            )),
        }
    }

    fn compare_types(&mut self) {
        let old_types = own_types(self.old);
        let new_types = own_types(self.new);
        for (name, old_ty) in &old_types {
            let Some(new_ty) = new_types.get(name) else {
                self.breaks(Rule::Removed, format!("type {name} removed"));
                continue;
            };
            // written by the old version and read by the new one, and the other way around
            let mut old_to_new = TypeCheck::new(self.old, self.new, OLD, NEW)
                .strict_bounds(true)
                .compare_signatures(self.signatures);
            let mut new_to_old =
                TypeCheck::new(self.new, self.old, NEW, OLD).compare_signatures(self.signatures);
            let result = old_to_new
                .check(old_ty, new_ty)
                .and_then(|_| new_to_old.check(new_ty, old_ty));
            if let Err(e) = result {
                let rule = match old_to_new.rule.or(new_to_old.rule) {
                    Some(rule) => rule,
                    None if old_to_new.lowered => Rule::MaxLen,
                    None => Rule::Type,
                };
                let root = old_to_new.root.clone().or_else(|| new_to_old.root.clone());
                self.breaks_with_root(rule, e, root);
            }
            self.bounds_raised(&format!("type {name}"), new_to_old.narrower);
            old_to_new.unwrapped.append(&mut new_to_old.unwrapped);
            self.unwrapped(&format!("type {name}"), &old_to_new.unwrapped);
            self.compare_idents(name, old_ty, new_ty);
        }
        for name in new_types.keys() {
            if !old_types.contains_key(name) {
                self.adds(format!("type {name} added"));
            }
        }
    }

    /// Max lengths the new version raised (or removed): compatible, everything the old version writes is still
    /// read, but values the new version writes beyond the old bound are rejected by the old version.
    fn bounds_raised(&mut self, path: &str, narrower: Vec<String>) {
        for msg in narrower {
            self.adds(format!("{path}: max length raised ({msg})"));
        }
    }

    /// A type became a transparent wrapper of it (DER-18, `u64` -> `ww_si::Fixed<..., u64, ...>`) or the other way
    /// around: compatible, the same bytes. Found in both directions, reported once, from the old version to the new.
    fn unwrapped(&mut self, path: &str, unwrapped: &[Unwrapped]) {
        for u in unwrapped {
            let msg = format!("{path}: {}", u.describe(OLD));
            if !self.compatible.contains(&msg) {
                self.compatible.push(msg);
            }
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
                // not recorded in a model from before EVO-10
                if let (Some(old_fv), Some(new_fv)) = (&old.future_version, &new.future_version)
                    && old_fv != new_fv
                {
                    let describe = |fv: &FutureVersionOwned| match fv {
                        FutureVersionOwned::Off => "none".to_string(),
                        FutureVersionOwned::Variant { ident, bits } => {
                            format!("{ident}(u{})", bits.unwrap_or(32))
                        }
                    };
                    self.breaks(
                        Rule::CatchAll,
                        format!(
                            "{name}: catch-all for newer variants changed from {} to {} (wire-compatible, but \
                             breaks Rust code)",
                            describe(old_fv),
                            describe(new_fv)
                        ),
                    );
                }
                for new_variant in &new.variants {
                    let path = format!("{name}::{}", new_variant.ident);
                    match old
                        .variants
                        .iter()
                        .find(|v| v.discriminant == new_variant.discriminant)
                    {
                        Some(old_variant) => {
                            if old_variant.ident != new_variant.ident {
                                self.breaks(Rule::Renamed, format!(
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
                        None => {
                            self.adds(format!("{path} added"));
                            self.check_since(&path, new_variant.since)
                        }
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
            self.breaks(
                Rule::FieldKind,
                format!("{name}: {old_kind} changed to {new_kind} (breaks Rust code)"),
            );
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
                        self.adds(format!("{path} added into unused padding bits"));
                    } else {
                        self.adds(format!("{path} added"));
                    }
                    self.check_since(&path, new_field.since);
                }
            }
            return;
        }
        for (i, (old_field, new_field)) in old.iter().zip(new).enumerate() {
            if old_field.ident != new_field.ident {
                self.breaks(
                    Rule::Renamed,
                    format!(
                        "{name}: field {} renamed to {} (wire-compatible, but breaks Rust code)",
                        field_name(i, old_field),
                        field_name(i, new_field)
                    ),
                );
            }
        }
        for (i, new_field) in new.iter().enumerate().skip(old.len()) {
            let path = format!("{name}: field {}", field_name(i, new_field));
            self.adds(format!("{path} added"));
            self.check_since(&path, new_field.since);
        }
    }

    /// The crate's wire version (EVO-24, EVO-26), when either side has one, listed. Whether it is bumped enough, or
    /// goes back, is [Report::check_wire]'s.
    fn compare_wire(&mut self) {
        match (self.old_wire, self.new_wire) {
            (Some(old), Some(new)) if old != new => {
                self.compatible.push(format!("wire version {old} -> {new}"))
            }
            (None, Some(new)) => self.compatible.push(format!("wire version {new} declared")),
            _ => {}
        }
    }

    /// Dependency crates (by crate id) whose wire version moved (EVO-26, EVO-27): to another line, a new wire line of
    /// this crate too, even where no trait or type compared found the break; to a newer revision of the same line, a
    /// new revision, as what this crate's devices carry grew. A dependency that declares none on either side is
    /// compared by its crate version, in the traits and types using it.
    fn compare_dependency_wires(&mut self) {
        for (new_idx, new_crate) in self.new.ext_crates.iter().enumerate().skip(1) {
            let Some(old_idx) = self
                .old
                .ext_crates
                .iter()
                .skip(1)
                .position(|c| c.crate_id == new_crate.crate_id)
            else {
                continue;
            };
            let (Some(old), Some(new)) = (
                self.old.crate_wire(old_idx as u32 + 1),
                self.new.crate_wire(new_idx as u32),
            ) else {
                continue;
            };
            let crate_id = &new_crate.crate_id;
            let line = |message: String| DependencyLine {
                crate_id: crate_id.clone(),
                message,
            };
            match WireBump::between(old, new) {
                WireBump::Line => self.wire_lines.push(line(format!(
                    "{crate_id} on a new wire line, {old} -> {new}"
                ))),
                WireBump::Revision => self.adds(format!("{crate_id} wire {old} -> {new}")),
                WireBump::None if !old.same_line(&new) => self.wire_lines.push(line(format!(
                    "{crate_id} on an older wire line, {old} -> {new}"
                ))),
                WireBump::None => {}
            }
        }
    }

    /// Additions are expected to be marked with `#[since]` naming the wire revision they first appeared in (EVO-24):
    /// the new wire version, which must then be newer than the old one. One newer than the new wire version is
    /// reported by [Self::since_not_newer], for every item. Without a wire version on the new side (a crate that
    /// declares none), only a missing `#[since]` is reported.
    fn check_since(&mut self, path: &str, since: Option<WireVersion>) {
        let expected = match (self.old_wire, self.new_wire) {
            (Some(old), Some(new)) if new <= old => Some((old.next_revision(), " and a wire bump")),
            (_, Some(new)) => Some((new, "")),
            (_, None) => None,
        };
        match (since, expected) {
            (None, Some((expected, bump))) => self
                .warnings
                .push(format!("{path} is added without #[since = \"{expected}\"]{bump}")),
            (None, None) => self.warnings.push(format!("{path} is added without #[since]")),
            (Some(since), Some((expected, bump)))
                if self.old_wire.is_some_and(|old| since <= old) =>
            {
                self.warnings.push(format!(
                    "{path} is added with #[since = \"{since}\"], expected #[since = \"{expected}\"]{bump}"
                ))
            }
            (Some(_), _) => {}
        }
    }

    /// A `#[since]` newer than the API's wire version, on any of its resources, fields or variants, added or not: a
    /// host built with it refuses that item on every device of this version, so [Report::check_wire] fails on each
    /// one returned. Markers are never rewritten on a new wire line, an older one counts as present
    /// (`wire_weaver_client`'s `has_since`).
    fn since_not_newer(&mut self) -> Vec<String> {
        let mut marked: Vec<(String, Option<WireVersion>)> = vec![];
        for (name, level) in own_traits(self.new) {
            for item in &level.items {
                marked.push((format!("{name}::{}", item.ident), item.since));
            }
        }
        let with_fields = |marked: &mut Vec<_>, prefix: &str, f: &FieldsOwned| {
            for (i, field) in fields(f).iter().enumerate() {
                marked.push((
                    format!("{prefix}: field {}", field_name(i, field)),
                    field.since,
                ));
            }
        };
        for (name, ty) in own_types(self.new) {
            match ty {
                TypeOwned::Struct(s) => with_fields(&mut marked, name, &s.fields),
                TypeOwned::Enum(e) => {
                    for variant in &e.variants {
                        let path = format!("{name}::{}", variant.ident);
                        marked.push((path.clone(), variant.since));
                        with_fields(&mut marked, &path, &variant.fields);
                    }
                }
                _ => {}
            }
        }
        let Some(new) = self.new_wire else {
            return vec![];
        };
        marked
            .into_iter()
            .filter_map(|(path, since)| {
                let since = since.filter(|since| *since > new)?;
                Some(format!(
                    "{path} has #[since = \"{since}\"], newer than the API's wire version {new}"
                ))
            })
            .collect()
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

#[cfg(all(test, feature = "source"))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    mod args;
    mod both_bumps;
    mod by_meaning;
    mod root_cause;
    mod transparent;
    mod wire_lines;
    mod ww_si_fixed;

    /// Load a crate bundle from source, as `ww api save` would, by writing a throwaway crate.
    fn load(version: &str, lib_rs: &str) -> ApiBundleOwned {
        load_with(version, lib_rs, "")
    }

    /// [load] with `dependencies`, lines of the `[dependencies]` section.
    /// The API crate of `lib_rs` with `dependencies`, loaded from source once per test: a load parses its
    /// dependencies' source too (~0.3 s with `ww_si`), and tests compare the same version in both directions.
    fn load_with(version: &str, lib_rs: &str, dependencies: &str) -> ApiBundleOwned {
        thread_local! {
            static LOADED: std::cell::RefCell<std::collections::HashMap<(String, String, String), ApiBundleOwned>> =
                Default::default();
        }
        let key = (
            version.to_string(),
            lib_rs.to_string(),
            dependencies.to_string(),
        );
        if let Some(bundle) = LOADED.with(|l| l.borrow().get(&key).cloned()) {
            return bundle;
        }
        let bundle = load_uncached(version, lib_rs, dependencies);
        LOADED.with(|l| l.borrow_mut().insert(key, bundle.clone()));
        bundle
    }

    fn load_uncached(version: &str, lib_rs: &str, dependencies: &str) -> ApiBundleOwned {
        // under the workspace, so the walker finds its Cargo.lock and [patch]es for git dependencies (exact_units)
        let dir: PathBuf = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/ww_evolution_tests")
            .join(format!(
                "{version}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"test_api\"\nversion = \"{version}\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}"
            ),
        )
        .unwrap();
        let lib_rs = format!("use wire_weaver::prelude::*;\n\n{lib_rs}");
        std::fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
        let bundle = wire_weaver_core::load_crate(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        bundle
    }

    /// `src` with its API root at wire version `wire` instead of `w1.0`.
    fn wire(src: &str, wire: &str) -> String {
        src.replace("wire = \"w1.0\"", &format!("wire = \"{wire}\""))
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

    /// API-20: an id is part of what a trait is to hosts and devices, it may be assigned but never changed or removed.
    #[test]
    fn ww_global_id_change_is_breaking() {
        let with_id = |id: Option<u32>| {
            let mut bundle = load("0.1.0", API);
            for location in &mut bundle.traits {
                if let ApiLevelLocationOwned::InLine { level, .. } = location {
                    level.compact_id = id.map(shrink_wrap::UNib32);
                }
            }
            bundle
        };
        let check = |old, new| compare(&with_id(old), &with_id(new)).unwrap();
        assert_breaking(
            &check(Some(70), Some(71)),
            "ww_global id changed from 70 to 71",
        );
        assert_breaking(&check(Some(70), None), "ww_global id 70 gone");
        let assigned = check(None, Some(70));
        assert_eq!(assigned.change, Change::Compatible, "{assigned:?}");
        assert!(
            assigned
                .compatible
                .iter()
                .any(|c| c.contains("ww_global id 70 assigned"))
        );
        assert_eq!(check(Some(70), Some(70)).change, Change::None);
        let differences = diff::diff(&with_id(Some(70)), &with_id(Some(71))).unwrap();
        assert!(
            differences.iter().any(|d| matches!(
                d,
                diff::Difference::Changed {
                    what: "ww_global id",
                    ..
                }
            )),
            "{differences:?}"
        );
    }

    const API: &str = r#"
        #[ww_api_root(wire = "w1.0")]
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
        let new = wire(
            &API.replace(
                "fn set(c: Coord);",
                "fn set(c: Coord);\n#[since = \"w1.1\"]\nfn led_off();",
            ),
            "w1.1",
        );
        let report = compare(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(
            report.compatible,
            ["wire version w1.0 -> w1.1", "Api::led_off added"]
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.check_version().is_ok());

        let unmarked = new.replace("#[since = \"w1.1\"]", "");
        let report = compare(&load("0.1.0", API), &load("0.1.1", &unmarked)).unwrap();
        assert_eq!(report.change, Change::Compatible);
        assert_eq!(
            report.warnings,
            ["Api::led_off is added without #[since = \"w1.1\"]"]
        );

        // the wire revision not bumped: the marker that is expected, and the bump
        let not_bumped = unmarked.replace("wire = \"w1.1\"", "wire = \"w1.0\"");
        let report = compare(&load("0.1.0", API), &load("0.1.1", &not_bumped)).unwrap();
        assert_eq!(
            report.warnings,
            ["Api::led_off is added without #[since = \"w1.1\"] and a wire bump"]
        );
        let old_marker = new.replace("w1.1", "w1.0");
        let report = compare(&load("0.1.0", API), &load("0.1.1", &old_marker)).unwrap();
        assert_eq!(
            report.warnings,
            [
                "Api::led_off is added with #[since = \"w1.0\"], expected #[since = \"w1.1\"] and a wire bump"
            ]
        );
    }

    /// A `#[since]` above the API's wire version is reported on every item, added or kept; one from an older line
    /// is fine after the API moved to a new line (markers are never rewritten).
    #[test]
    fn since_newer_than_the_wire_version() {
        let marked = |item: &str, field: &str, variant: &str| {
            API.replace("fn led_on();", &format!("{item}\nfn led_on();"))
                .replace(
                    "pub y: u8,",
                    &format!("pub y: u8,\n{field}\n#[default = 0]\npub z: u8,"),
                )
                .replace("B(u8),", &format!("B(u8),\n{variant}\nC,"))
        };
        let old = wire(
            &marked(
                "#[since = \"w1.1\"]",
                "#[since = \"w1.2\"]",
                "#[since = \"w1.2\"]",
            ),
            "w1.2",
        );
        // w1.2 -> w2.0: older markers stay as they are, no warnings
        let report = compare(
            &load("0.5.2", &old),
            &load("0.6.0", &old.replace("wire = \"w1.2\"", "wire = \"w2.0\"")),
        )
        .unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(
            report.since_too_new.is_empty(),
            "{:?}",
            report.since_too_new
        );
        report.check_wire().unwrap();

        let new = wire(
            &marked(
                "#[since = \"w2.1\"]",
                "#[since = \"w2.2\"]",
                "#[since = \"w3.0\"]",
            ),
            "w2.0",
        );
        let report = compare(&load("0.5.2", &old), &load("0.6.0", &new)).unwrap();
        let mut too_new = report.since_too_new.clone();
        too_new.sort();
        assert_eq!(
            too_new,
            [
                "Api::led_on has #[since = \"w2.1\"], newer than the API's wire version w2.0",
                "Coord: field `z` has #[since = \"w2.2\"], newer than the API's wire version w2.0",
                "Mode::C has #[since = \"w3.0\"], newer than the API's wire version w2.0",
            ]
        );
        // EVO-27: each one fails the check
        let e = report.check_wire().unwrap_err();
        assert!(e.contains("Mode::C has #[since = \"w3.0\"]"), "{e}");

        // an addition marked newer than the new wire version: reported once
        let added = wire(
            &API.replace(
                "fn set(c: Coord);",
                "fn set(c: Coord);\n#[since = \"w1.2\"]\nfn led_off();",
            ),
            "w1.1",
        );
        let report = compare(&load("0.1.0", API), &load("0.1.1", &added)).unwrap();
        assert_eq!(
            report.since_too_new,
            ["Api::led_off has #[since = \"w1.2\"], newer than the API's wire version w1.1"]
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.check_wire().is_err());
    }

    #[test]
    fn max_length_raised_is_compatible_lowered_is_breaking() {
        let api = |data: &str, name: &str, field: &str| {
            format!(
                r#"
                #[ww_api_root(wire = "w1.0")]
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
                #[ww_api_root(wire = "w1.0")]
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
            "fn led_on();\n#[since = \"w1.1\"]\nfn led_off();",
        );
        let report = self::report(API, &new);
        assert_eq!(
            report.breaking,
            [Finding::new(
                Rule::Moved,
                "Api::set moved from id 1 to 2 (resources are identified by position, add new ones at the end)"
            )]
        );
    }

    #[test]
    fn changed_argument_is_breaking() {
        let new = API.replace("fn set(c: Coord);", "fn set(c: u8);");
        assert_breaking(&report(API, &new), "Api::set: argument `c`");
    }

    #[test]
    fn struct_fields() {
        let with_default = wire(
            &API.replace(
                "pub y: u8,",
                "pub y: u8,\n#[default = None]\n#[since = \"w1.1\"]\npub z: Option<u8>,",
            ),
            "w1.1",
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
        let v2 = wire(
            &v1.replace(
                "pub f: bool,",
                "pub f: bool,\n#[since = \"w1.1\"]\npub g: u3,",
            ),
            "w1.1",
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
        let v2 = wire(
            &v1.replace(
                "pub f: bool,",
                "pub f: bool,\n#[since = \"w1.1\"]\npub g: u7,",
            ),
            "w1.1",
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
            "pub s: String,\n#[default = None]\n#[since = \"w1.1\"]\npub z: Option<u8>,",
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
        let new = API.replace("B(u8),", "Bb(u8),");
        assert_breaking(&report(API, &new), "Mode::B renamed to Bb");
    }

    /// EVO-10: an older reader gets a variant appended to an enum with the catch-all (SW-47) as `Unknown(n)`.
    #[test]
    fn enum_variant_appended_to_the_catch_all() {
        // Mode is Unsized, it has Unknown(u32)
        for appended in ["C,", "C(u16),", "C { a: u8 },"] {
            let new = API.replace("B(u8),", &format!("B(u8),\n{appended}"));
            let report = compare(&load("0.1.0", API), &load("0.1.1", &new)).unwrap();
            assert_eq!(report.change, Change::Compatible, "{appended}: {report:?}");
            assert!(report.check_version().is_ok(), "patch before 1.0");
        }
        let named =
            |api: &str| api.replace("ww_repr = u4)", "ww_repr = u4, future_version = Newer)");
        let new = named(&API.replace("B(u8),", "B(u8),\nC,"));
        assert_eq!(report(&named(API), &new).change, Change::Compatible);
        let written = |api: &str| {
            api.replace("ww_repr = u4)", "ww_repr = u4, self_describing)")
                .replace("B(u8),", "B(u8),\n#[future_version]\nLater(u32),")
        };
        let new = written(API).replace("B(u8),", "B(u8),\nC,");
        assert_eq!(report(&written(API), &new).change, Change::Compatible);

        // the old version reads Mode::B as the new C
        let inserted = API.replace("A,", "A,\nC(u8),");
        assert_breaking(&report(API, &inserted), "variant B moved");
        let reordered = API.replace(
            "A,\n            B(u8),",
            "B(u8),\n            A,\n            C,",
        );
        assert_breaking(&report(API, &reordered), "variant A moved");
        // a self_describing enum can't skip a newer variant's fields
        let new = written(API).replace("B(u8),", "B(u8),\nC(u8),");
        assert_breaking(
            &report(&written(API), &new),
            "its catch-all Later(u32) can't skip the variant's fields",
        );
        // DER-25: a u8 catch-all holds a newer discriminant up to 255
        let narrow = |api: &str, ty: &str| {
            api.replace("ww_repr = u4)", "ww_repr = unib32, self_describing)")
                .replace(
                    "B(u8),",
                    &format!("B(u8),\n#[future_version]\nLater({ty}),"),
                )
        };
        let old = narrow(API, "u8");
        let new = old.replace("B(u8),", "B(u8),\nC = 255,");
        assert_eq!(report(&old, &new).change, Change::Compatible);
        let new = old.replace("B(u8),", "B(u8),\nC = 256,");
        assert_breaking(
            &report(&old, &new),
            "its discriminant 256 doesn't fit the catch-all Later(u8)",
        );
        assert_breaking(
            &report(&old, &narrow(API, "u16")),
            "from Later(u8) to Later(u16)",
        );
    }

    #[test]
    fn enum_variant_appended_without_the_catch_all() {
        let without = [
            ("ww_repr = u4)", "ww_repr = u4, no_future_version)"),
            ("ww_repr = u4)", "ww_repr = u4, self_describing)"),
        ];
        for (from, to) in without {
            let old = API.replace(from, to).replace("B(u8),", "B(u8),\nC,");
            let new = old.replace("C,", "C,\nD,");
            assert_breaking(
                &report(&old, &new),
                "variant D is unknown to the old version, which has no catch-all for it",
            );
        }
        // a model from before EVO-10 doesn't say whether the old version has the catch-all
        let new = API.replace("B(u8),", "B(u8),\nC,");
        let mut old = load("0.1.0", API);
        for location in &mut old.types {
            if let TypeLocationOwned::InLine {
                ty: TypeOwned::Enum(e),
                ..
            } = location
            {
                e.future_version = None;
            }
        }
        let report = compare(&old, &load("0.1.0", &new)).unwrap();
        assert_breaking(
            &report,
            "the API model is from before EVO-10, save its snapshot again",
        );
    }

    #[test]
    fn catch_all_changed_breaks_rust_code() {
        let off = API.replace("ww_repr = u4)", "ww_repr = u4, no_future_version)");
        let named = API.replace("ww_repr = u4)", "ww_repr = u4, future_version = Newer)");
        assert_breaking(
            &report(API, &off),
            "Mode: catch-all for newer variants changed from Unknown(u32) to none",
        );
        assert_breaking(
            &report(&off, API),
            "Mode: catch-all for newer variants changed from none to Unknown(u32)",
        );
        assert_breaking(&report(API, &named), "from Unknown(u32) to Newer(u32)");
        // the fourth variant of a u2 takes the last discriminant, there is none left for Unknown
        let three = API
            .replace("ww_repr = u4)", "ww_repr = u2)")
            .replace("B(u8),", "B(u8),\nC,");
        let four = three.replace("C,", "C,\nD,");
        assert_breaking(
            &report(&three, &four),
            "Mode: catch-all for newer variants changed from Unknown(u32) to none",
        );
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
                "fn set(c: Coord) -> u8;\n#[since = \"w1.1\"]\nfn led_off();",
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
                #[ww_api_root(wire = "w1.0")]
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

    /// Making an item or a `ww_impl!` optional is compatible, making it required again or moving it to another group
    /// breaks servers that leave it out (API-16, API-54).
    #[test]
    fn optional_items_and_traits() {
        let api = |diag: &str, read: &str| {
            format!(
                r#"
                #[ww_api_root(wire = "w1.0")]
                pub trait Api {{
                    {diag}
                    ww_impl!(diag: Diag);
                }}

                #[ww_trait]
                pub trait Diag {{
                    {read}
                    fn read() -> u8;
                }}
                "#
            )
        };
        let report = report(&api("", ""), &api("#[optional]", "#[optional]"));
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(
            report.compatible,
            [
                "Api::diag: made #[optional] (a device may leave it out, a client gets Unimplemented)",
                "Diag::read: made #[optional] (a device may leave it out, a client gets Unimplemented)"
            ]
        );
        let report = self::report(&api("#[optional]", ""), &api("#[optional(debug)]", ""));
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_breaking(
            &self::report(&api("#[optional]", ""), &api("", "")),
            "Api::diag: no longer #[optional]",
        );
        assert_breaking(
            &self::report(&api("#[optional(a)]", ""), &api("#[optional(b)]", "")),
            "Api::diag: #[optional(a)] became #[optional(b)]",
        );
    }

    /// `#[claim]`, `#[shares]` and `#[label]` (SRV-34) changing is compatible and reported.
    #[test]
    fn meta_changes_are_compatible() {
        let api = |port: &str, trait_attr: &str| {
            format!(
                r#"
                #[ww_api_root(wire = "w1.0")]
                pub trait Api {{
                    {port}
                    ww_impl!(port: Port);
                }}

                #[ww_trait]
                {trait_attr}
                pub trait Port {{
                    fn write();
                }}
                "#
            )
        };
        let report = report(
            &api("", ""),
            &api(
                "#[claim(shared)] #[shares(pins = \"PA9\")] #[label(\"Debug\")]",
                "#[claim]",
            ),
        );
        assert_eq!(report.change, Change::Compatible, "{report:?}");
        assert_eq!(
            report.compatible,
            [
                "Api::port: not claimable became #[claim(shared)]",
                "Api::port: #[shares] changed",
                "Api::port: #[label] changed",
                "trait Port: not claimable became #[claim(exclusive)]",
            ]
        );
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
                #[ww_api_root(wire = "w1.0")]
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
        for (version, bundle) in wire_weaver_snapshots::all() {
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
